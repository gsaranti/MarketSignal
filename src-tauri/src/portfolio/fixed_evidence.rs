//! The fixed evidence set from big-run attempt 6 — the verification substrate the
//! local-model-call slices are admitted against (fix list §8.1 / §8.2, ruled
//! 2026-09-15; `docs/verification/2026-09-16-ledger-conditions-and-action-packet.md`).
//!
//! Each fixture under `fixtures/attempt-6/` is **reconstructed, not replayed**:
//! reduced from a holding's persisted row (the authored ledger conditions, the
//! verdict digest, the engine output assembled from the audit, the distilled
//! research text) with the position's economics replaced by synthetic values
//! (ruled 2026-09-16, F8). The offline tests replay the persisted drafts through
//! the 6g seam and render the action packet; the `#[ignore]`d live harness issues
//! the interpretation and action calls against the local daemon on the same
//! evidence and prints the per-holding table the §8.2 admission read is made
//! from.

use super::engine::{self, CompanyFinancials, EngineOutput};
use super::pipeline::{
    self, action_user_prompt, interpretation_user_prompt, role_risk_user_prompt, tax_caveat,
    validate_ledger_rewrite_with_research, ActionInput, ActionSubject, HoldingAnalyst,
    InterpretationInput, RoleRiskInput,
};
use super::{
    Action, AssetClass, ContinuityStamps, FalsifierDraft, LedgerDraft, MonitorScenario,
    QuantCoreDraft, LedgerBranch, ScenarioKind, StatementBasis, ThesisLedger, TriggerDraft,
    VerdictDisposition,
};
use crate::portfolio::dossier::HouseView;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
struct Fixture {
    symbol: String,
    origin: String,
    asset_class: AssetClass,
    is_fund: bool,
    statement_basis: Option<StatementBasis>,
    /// The balance-sheet instants' equity source the run stamped — present on
    /// every holding whose computed metrics carry debt / equity or price / book,
    /// since the live dossier stamps the source wherever an equity line reached
    /// the engine (fix list 8.5, ruled 2026-09-17: the harness must not render
    /// a computed D/E beside a no-balance-sheet line the run never showed).
    equity_source: Option<crate::portfolio::EquitySource>,
    spot: f64,
    /// The dated close the run authored against — seeded as the one daily
    /// close of the reconstructed financials, so the market-data series
    /// resolve an observation in the live harness.
    authoring_close: engine::DatedValue,
    synthetic_position: SyntheticPosition,
    conditions: Vec<FixtureCondition>,
    /// The ledger's prose as the run persisted it — the current thesis, the
    /// three monitor rows and the two must-lines — so the action packet's
    /// THESIS and SCENARIOS sections render on the harness (`portfolio-v41`,
    /// ruled 2026-09-17; extracted from the same rows on 2026-09-17).
    ledger_prose: LedgerProse,
    disposition: VerdictDisposition,
    engine_output: EngineOutput,
    research_combined: String,
}

#[derive(Deserialize)]
struct SyntheticPosition {
    quantity: f64,
    cost_basis: f64,
    market_value: f64,
}

#[derive(Deserialize)]
struct LedgerProse {
    current_thesis: String,
    monitor: Vec<ProseScenario>,
    what_must_improve: String,
    what_must_not_break: String,
}

#[derive(Deserialize)]
struct ProseScenario {
    scenario: ScenarioKind,
    conditions: String,
    probability_pct: f64,
}

#[derive(Deserialize)]
struct FixtureCondition {
    role: String,
    family: Option<String>,
    statement: String,
    technology_class: bool,
    quant: Option<QuantCoreDraft>,
    /// `kept`, `qualitative`, or `downgraded:<class>`.
    expect: String,
}

const FIXTURES: [(&str, &str); 6] = [
    ("TSLA", include_str!("fixtures/attempt-6/tsla.json")),
    ("PSX", include_str!("fixtures/attempt-6/psx.json")),
    ("SPMO", include_str!("fixtures/attempt-6/spmo.json")),
    ("ARKF", include_str!("fixtures/attempt-6/arkf.json")),
    ("DIA", include_str!("fixtures/attempt-6/dia.json")),
    ("PGNY", include_str!("fixtures/attempt-6/pgny.json")),
];
const HOUSE_VIEW: &str = include_str!("fixtures/attempt-6/house-view.json");

fn fixtures() -> Vec<Fixture> {
    FIXTURES
        .iter()
        .map(|(sym, json)| {
            let f: Fixture = serde_json::from_str(json).unwrap_or_else(|e| panic!("{sym}: {e}"));
            assert_eq!(f.symbol, *sym);
            assert!(f.origin.starts_with("reconstructed, not replayed"), "{sym}: {}", f.origin);
            f
        })
        .collect()
}

/// The persisted conditions as the draft the model returned — the shape 6g
/// validated on the run, so the replay reads the same input.
fn draft_of(f: &Fixture) -> LedgerDraft {
    let mut draft = pipeline::stub_ledger_draft(None, &f.symbol, false);
    draft.falsifiers = f
        .conditions
        .iter()
        .filter(|c| c.role == "falsifier")
        .map(|c| FalsifierDraft {
            statement: c.statement.clone(),
            quant: c.quant.clone(),
            technology_class: c.technology_class,
            tripped: false,
        })
        .collect();
    draft.triggers = f
        .conditions
        .iter()
        .filter(|c| c.role == "trigger")
        .map(|c| TriggerDraft {
            statement: c.statement.clone(),
            family: c.family.clone().unwrap_or_else(|| "trim".into()),
            quant: c.quant.clone(),
            fired: false,
        })
        .collect();
    draft
}

/// The holding's ledger as the action packet reads it: the persisted prose
/// with no conditions or drivers (the packet renders neither), the original
/// thesis standing in for the current one, the scenario targets and the band
/// relation absent — the fields the packet does not read.
fn ledger_of(f: &Fixture) -> ThesisLedger {
    ThesisLedger {
        branch: LedgerBranch::Priced,
        original_thesis: f.ledger_prose.current_thesis.clone(),
        current_thesis: f.ledger_prose.current_thesis.clone(),
        key_drivers: vec![],
        monitor: f
            .ledger_prose
            .monitor
            .iter()
            .map(|s| MonitorScenario {
                scenario: s.scenario,
                conditions: s.conditions.clone(),
                probability_pct: s.probability_pct,
                engine_target: None,
            })
            .collect(),
        what_must_improve: f.ledger_prose.what_must_improve.clone(),
        what_must_not_break: f.ledger_prose.what_must_not_break.clone(),
        conditions: vec![],
        authored_band_relation: None,
    }
}

fn stamps_of(f: &Fixture) -> ContinuityStamps {
    ContinuityStamps {
        statement_basis: f.statement_basis,
        equity_source: f.equity_source,
    }
}

/// A holding dossier reconstructed at bounded fidelity: identity, the synthetic
/// position, the spot, the stamped basis, the authoring close as the one dated
/// price print, persisted options readings, and the house view. The statement rows and the fund context are not
/// persisted and stay absent, so the filing-cadence series carry no observation
/// and the fund-specific prompt sections do not render.
fn dossier_of(f: &Fixture, tax_sensitive: bool) -> super::dossier::HoldingDossier {
    let financials = CompanyFinancials {
        symbol: f.symbol.clone(),
        current_price: Some(f.spot),
        statement_basis: f.statement_basis,
        equity_source: f.equity_source,
        daily_closes: vec![f.authoring_close.clone()],
        ..Default::default()
    };
    let mut d = pipeline::tests::dossier(f.asset_class, financials);
    // The generic dossier carries sample options readings. These fixtures
    // persist the actual readings, which must reach the interpretation packet.
    if let VerdictDisposition::Priced(graded) = &f.disposition {
        d.options_signal = graded.options_signal.clone();
    } else {
        d.options_signal = super::OptionsSignal {
            put_call_volume: None,
            put_call_open_interest: None,
            implied_volatility: None,
            iv_skew: None,
        };
    }
    d.position.symbol = f.symbol.clone();
    d.position.description = String::new();
    d.position.quantity = f.synthetic_position.quantity;
    d.position.cost_basis = f.synthetic_position.cost_basis;
    d.position.market_value = f.synthetic_position.market_value;
    d.position.current_price = Some(f.spot);
    d.profile.tax_sensitive = tax_sensitive;
    d.house_view = serde_json::from_str::<HouseView>(HOUSE_VIEW).expect("house view");
    // The attempt-6 run date, the fixtures' provenance (`portfolio-v43`).
    d.analysis_date = "2026-09-16".into();
    d
}

fn expected_class(expect: &str) -> Option<&str> {
    expect.strip_prefix("downgraded:")
}

// ---- The synthetic role/risk fixture (`portfolio-v42`, ruled 2026-09-17) ----

/// The one role/risk case the harness carries — synthetic, not reconstructed:
/// attempt 6 priced all three of its funds, so no persisted role/risk row
/// exists. A total bond market ETF at a plausible spot, its reported asset
/// class routing it to the role/risk readout through the fund engine itself, a
/// hand-written research summary, the fixed set's house view, and a
/// hand-written Treasury positioning line; labelled synthetic wherever it
/// prints (the README, the dump, the live table).
struct SyntheticRoleRisk {
    dossier: super::dossier::HoldingDossier,
    readout: super::fund::RoleRiskReadout,
    research: &'static str,
}

/// The synthetic research summary — plain fund facts in the distillation's
/// register, carrying no banned word so the lexicon scan reads the app's own
/// sentences alone.
const SYNTHETIC_ROLE_RISK_RESEARCH: &str = "The Vanguard Total Bond Market ETF tracks the \
Bloomberg U.S. Aggregate Float Adjusted Index, holding about 11,000 investment-grade, taxable \
U.S. dollar bonds: roughly 45% U.S. Treasuries and agencies, 25% corporate bonds and 20% \
agency mortgage-backed securities, with an average duration near six years and an average \
credit quality of AA. The fund's expense ratio of 0.03% is among the lowest in its category, \
and its 30-day SEC yield stood near 4.4% in early September 2026. Fund flows were positive \
through the summer as investors added duration ahead of the Federal Reserve's September \
meeting; the fund's price fell about 1.5% over the four weeks to 2026-09-14 as the ten-year \
Treasury yield rose toward 5%. No change to the index methodology, the management team or \
the distribution policy was reported in the period.";

/// Twenty-one hand-written sessions ending at the spot, so the market-data
/// series resolve an observation and the fund engine computes a volatility.
const SYNTHETIC_ROLE_RISK_CLOSES: [(&str, f64); 21] = [
    ("2026-08-14", 73.42), ("2026-08-17", 73.38), ("2026-08-18", 73.51), ("2026-08-19", 73.30),
    ("2026-08-20", 73.22), ("2026-08-21", 73.09), ("2026-08-24", 73.15), ("2026-08-25", 72.98),
    ("2026-08-26", 72.87), ("2026-08-27", 72.94), ("2026-08-28", 72.80), ("2026-08-31", 72.66),
    ("2026-09-01", 72.71), ("2026-09-02", 72.58), ("2026-09-03", 72.49), ("2026-09-04", 72.55),
    ("2026-09-08", 72.40), ("2026-09-09", 72.33), ("2026-09-10", 72.45), ("2026-09-11", 72.36),
    ("2026-09-14", 72.38),
];

fn synthetic_role_risk_fixture() -> SyntheticRoleRisk {
    use super::fund::{self, FundContext, FundData, FundEngineInputs, FundEngineVerdict};
    let fund = FundData {
        symbol: "BND".into(),
        name: Some("Vanguard Total Bond Market ETF".into()),
        asset_class: Some("Fixed Income".into()),
        expense_ratio: Some(0.0003),
        aum: Some(3.4e11),
        nav: Some(72.41),
        sector_weights: vec![],
        country_weights: vec![
            ("United States".into(), 0.94),
            ("Supranational".into(), 0.02),
            ("Canada".into(), 0.01),
        ],
        profile_is_fund: Some(true),
        profile_description: None,
        gaps: vec![],
    };
    let closes: Vec<engine::DatedValue> = SYNTHETIC_ROLE_RISK_CLOSES
        .iter()
        .map(|(date, value)| engine::DatedValue { date: (*date).into(), value: *value })
        .collect();
    let spot = closes.last().map(|c| c.value).expect("closes");
    let financials = CompanyFinancials {
        symbol: "BND".into(),
        current_price: Some(spot),
        price_history: closes.iter().map(|c| c.value).collect(),
        daily_closes: closes,
        ttm_dividends_per_share: Some(2.61),
        ..Default::default()
    };
    let as_of = chrono::NaiveDate::from_ymd_opt(2026, 9, 16).expect("a date");
    let readout = match fund::analyze_fund(&FundEngineInputs {
        fund: &fund,
        financials: &financials,
        sector_pe: &[],
        sector_pe_history: &std::collections::HashMap::new(),
        rates: &pipeline::tests::rates(),
        as_of,
    }) {
        FundEngineVerdict::RoleRiskOnly(r) => *r,
        other => panic!("the synthetic bond fund must route to role/risk: {other:?}"),
    };
    let mut d = pipeline::tests::dossier(AssetClass::Etf, financials);
    d.position.symbol = "BND".into();
    d.position.description = "VANGUARD TOTAL BOND MARKET ETF".into();
    d.position.quantity = 10.0;
    d.position.cost_basis = 730.0;
    d.position.market_value = spot * 10.0;
    d.position.current_price = Some(spot);
    d.options_signal = super::OptionsSignal {
        put_call_volume: None,
        put_call_open_interest: None,
        implied_volatility: None,
        iv_skew: None,
    };
    d.house_view = serde_json::from_str::<HouseView>(HOUSE_VIEW).expect("house view");
    d.analysis_date = "2026-09-16".into();
    d.put_call_backdrop = Some(crate::cboe::PutCallBackdrop {
        as_of: "2026-09-16".into(),
        total: Some(0.95),
        index: Some(1.21),
        equity: Some(0.62),
    });
    d.fund = Some(FundContext {
        fund,
        sector_pe: vec![],
        sector_pe_history: std::collections::HashMap::new(),
        as_of,
        positioning: Some(crate::data_sources::CotPositioning {
            contract: "10-Year US Treasury Note".into(),
            contract_code: "043602".into(),
            asset_class: "rates".into(),
            report_date: "2026-09-09".into(),
            open_interest: 4_800_000.0,
            spec_net: -650_000.0,
            spec_net_weekly_change: Some(-22_000.0),
            spec_pct_oi_long: Some(18.4),
            real_money_net: Some(1_200_000.0),
            real_money_net_weekly_change: Some(15_000.0),
        }),
    });
    SyntheticRoleRisk { dossier: d, readout, research: SYNTHETIC_ROLE_RISK_RESEARCH }
}

/// The synthetic case's debut input.
fn synthetic_role_risk_input(fx: &SyntheticRoleRisk) -> RoleRiskInput<'_> {
    RoleRiskInput {
        input_delta: &[],
        dossier: &fx.dossier,
        prior_ledger: None,
        readout: &fx.readout,
        ledger_eval: None,
        distilled: fx.research,
    }
}

/// The synthetic case's verdict and validated ledger off the stub's
/// interpretation — the pipeline's own assembly, so the action packet reads
/// what a run would have persisted.
fn synthetic_role_risk_verdict(fx: &SyntheticRoleRisk) -> (super::RoleRiskVerdict, ThesisLedger) {
    let interp = pipeline::StubAnalyst
        .interpret_role_risk(&synthetic_role_risk_input(fx))
        .expect("the stub interprets");
    let (ledger, _) = validate_ledger_rewrite_with_research(
        &interp.ledger,
        None,
        None,
        LedgerBranch::RoleRiskOnly,
        true,
        None,
        fx.dossier.financials.current_price,
        &HashSet::new(),
        true,
        ContinuityStamps::NONE,
    );
    (pipeline::role_risk_verdict_from_interpretation(&fx.readout, interp), ledger)
}

/// A stub that captures the role/risk message the pipeline renders — the
/// continuity variant only `analyze_holding` can build, since the delta, the
/// prior ledger's evaluation and the crossings are the pipeline's.
#[derive(Default)]
struct RoleRiskCapture {
    system: std::cell::RefCell<Option<String>>,
    user: std::cell::RefCell<Option<String>>,
}

impl HoldingAnalyst for RoleRiskCapture {
    fn interpret(&self, input: &InterpretationInput) -> anyhow::Result<super::Interpretation> {
        pipeline::StubAnalyst.interpret(input)
    }
    fn interpret_role_risk(&self, input: &RoleRiskInput) -> anyhow::Result<super::RoleRiskInterpretation> {
        let debut = input.dossier.prior_verdict.is_none();
        *self.system.borrow_mut() = Some(pipeline::role_risk_system_prompt(debut));
        *self.user.borrow_mut() = Some(role_risk_user_prompt(input));
        pipeline::StubAnalyst.interpret_role_risk(input)
    }
    fn decide_action(&self, input: &ActionInput) -> anyhow::Result<super::ActionDecision> {
        pipeline::StubAnalyst.decide_action(input)
    }
    fn fast_id(&self) -> String {
        "fast".into()
    }
    fn reasoner_id(&self) -> String {
        "reasoner".into()
    }
}

/// The synthetic case's continuity messages: a stub first run on 2026-09-03,
/// then the same fund with that verdict as its prior, the system and user
/// messages captured on the second run.
fn synthetic_role_risk_continuity_messages() -> (String, String) {
    let fx = synthetic_role_risk_fixture();
    let rates = pipeline::tests::rates();
    let (first, _) = pipeline::analyze_holding(&pipeline::StubAnalyst, &fx.dossier, &rates, "2026-09-03")
        .expect("the stub's first run");
    assert!(
        matches!(first.disposition, VerdictDisposition::RoleRiskOnly(_)),
        "the synthetic fund must take the role/risk branch: {:?}",
        first.disposition
    );
    let mut second = fx.dossier.clone();
    second.prior_verdict = Some(first);
    second.prior_vintage = Some("2026-09-03T14:00:00Z".into());
    second.position_delta = super::PositionDelta {
        change: super::PositionChange::Unchanged,
        prior_quantity: Some(10.0),
        prior_cost_basis: Some(730.0),
    };
    let capture = RoleRiskCapture::default();
    let _ = pipeline::analyze_holding(&capture, &second, &rates, "2026-09-17").expect("the second run");
    (
        capture.system.take().expect("the system prompt was captured"),
        capture.user.take().expect("the user message was captured"),
    )
}

#[test]
fn reconstructed_interpretation_uses_the_persisted_options_evidence() {
    for f in fixtures() {
        let VerdictDisposition::Priced(graded) = &f.disposition else { panic!("priced fixture") };
        let d = dossier_of(&f, true);
        let prompt = pipeline::interpretation_user_prompt(&InterpretationInput {
            input_delta: &[],
            dossier: &d,
            prior_ledger: None,
            engine: &f.engine_output,
            distilled: &f.research_combined,
            ledger_eval: None,
            pre_profit: None,
            tech_pre_flag: None,
            narrative: None,
        });
        let options = &graded.options_signal;
        // The OPTIONS ACTIVITY section (`portfolio-v40`): the persisted values
        // with their unit and polarity, no grade-input disclaimer.
        let activity = format!(
            "\nOPTIONS ACTIVITY\nput/call volume {:.3}, put/call open interest {:.3}, implied \
             volatility {:.3}, IV skew 0.000 (mean put IV minus mean call IV, in IV's decimal \
             unit; positive means puts are richer).\n",
            options.put_call_volume.expect("persisted volume"),
            options.put_call_open_interest.expect("persisted OI"),
            options.implied_volatility.expect("persisted IV"),
        );
        assert_eq!(options.iv_skew, Some(0.0), "{}: fixture skew", f.symbol);
        assert!(prompt.contains(&activity), "{}: {activity}", f.symbol);
        assert!(!prompt.contains("NOT a grade input"), "{}: {prompt}", f.symbol);
    }
}

#[test]
fn attempt_6_conditions_resolve_to_their_expected_6g_outcome() {
    for f in fixtures() {
        let draft = draft_of(&f);
        let (ledger, audit) = validate_ledger_rewrite_with_research(
            &draft,
            None,
            None,
            LedgerBranch::Priced,
            f.is_fund,
            None,
            Some(f.spot),
            &HashSet::new(),
            true,
            stamps_of(&f),
        );
        assert_eq!(ledger.conditions.len(), f.conditions.len(), "{}: never dropped", f.symbol);
        for expected in &f.conditions {
            let cond = ledger
                .conditions
                .iter()
                .find(|c| c.statement == expected.statement.trim())
                .unwrap_or_else(|| panic!("{}: '{}' persisted", f.symbol, expected.statement));
            match expected.expect.as_str() {
                "kept" => {
                    assert!(cond.quant.is_some(), "{}: '{}' kept quantitative: {:?}", f.symbol, cond.statement, cond.downgraded_reason);
                    assert!(cond.downgraded_reason.is_none());
                    assert!(cond.eval_state.is_some());
                }
                "qualitative" => {
                    assert!(cond.quant.is_none() && cond.downgraded_reason.is_none(), "{}: '{}'", f.symbol, cond.statement);
                }
                other => {
                    let class = expected_class(other).expect("downgraded:<class>");
                    let reason = cond.downgraded_reason.as_deref().unwrap_or_else(|| {
                        panic!("{}: '{}' should downgrade as {class}", f.symbol, cond.statement)
                    });
                    assert!(reason.starts_with(&format!("{class}:")), "{}: '{}' → {reason}", f.symbol, cond.statement);
                    assert!(cond.quant.is_none() && cond.eval_state.is_none());
                    assert!(audit.downgraded.iter().any(|d| d.contains(class)), "{:?}", audit.downgraded);
                }
            }
        }
    }
}

#[test]
fn attempt_6_action_packets_carry_no_account_economics_and_are_tax_invariant() {
    for f in fixtures() {
        let VerdictDisposition::Priced(graded) = &f.disposition else {
            panic!("{}: every attempt-6 holding priced", f.symbol)
        };
        let engine_set = engine::feasible_actions(f.engine_output.grade, &f.engine_output.hurdle, None, false);
        let ledger = ledger_of(&f);
        let render = |d: &super::dossier::HoldingDossier| {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded, engine: &f.engine_output, pre_profit: None, ledger: &ledger },
                engine_set: &engine_set,
                changes: None,
                profile: &d.profile,
            })
        };
        let taxable = render(&dossier_of(&f, true));
        let exempt = render(&dossier_of(&f, false));
        assert_eq!(taxable, exempt, "{}: the tax posture must not change the packet", f.symbol);
        let mut repriced = dossier_of(&f, true);
        repriced.position.cost_basis *= 3.0;
        repriced.position.quantity *= 2.0;
        repriced.position.market_value *= 2.0;
        assert_eq!(taxable, render(&repriced), "{}: account economics must not change the packet", f.symbol);
        // Case-insensitive on the whole packet: the app-rendered lines never carry
        // these, and the fixture's model-authored prose is scrubbed of them.
        let lower = taxable.to_lowercase();
        for absent in ACCOUNT_ECONOMICS_PHRASES.iter().copied().chain(["- tax", "quantity:"]) {
            assert!(!lower.contains(absent), "{}: {absent} leaked: {taxable}", f.symbol);
        }
        // The set once as one data line, the polarity gloss once (`portfolio-v41`).
        assert_eq!(taxable.matches("\nSUPPORTED ACTIONS (computed)\n").count(), 1, "{}", f.symbol);
        assert_eq!(taxable.matches("higher is better on every axis").count(), 1, "{}", f.symbol);
        // The app-appended caveat for the rung the run chose, on the synthetic P/L.
        let d = dossier_of(&f, true);
        let caveat = tax_caveat(&d.profile, &d.position, graded.action);
        let gain = d.position.market_value > d.position.cost_basis;
        match graded.action {
            Action::Trim | Action::SellAll if gain => assert_eq!(caveat, Some(pipeline::TAX_CAVEAT_GAIN), "{}", f.symbol),
            Action::Trim | Action::SellAll => assert_eq!(caveat, Some(pipeline::TAX_CAVEAT_LOSS), "{}", f.symbol),
            _ => assert_eq!(caveat, None, "{}", f.symbol),
        }
    }
}

/// The live read's prose diagnostics, printed beside a model-authored field and
/// never a gate: an account-economics phrase (fix list 3.2) and any banned
/// word (`portfolio-v41`, ruled 2026-09-17 F3 — the offline pin scans the app's
/// own sentences, so the model's prose is read here).
fn prose_diagnostics(label: &str, text: &str) {
    let lower = text.to_lowercase();
    if let Some(phrase) = ACCOUNT_ECONOMICS_PHRASES.iter().find(|p| lower.contains(**p)) {
        println!("    !! account-economics phrase in {label}: \"{phrase}\"");
    }
    let hits = banned_hits(text);
    if !hits.is_empty() {
        println!("    !! banned word in {label}: {hits:?}");
    }
}

/// Phrases a rationale carries when it argues from what the engine permits
/// rather than from the evidence — the 3.9 read's permission scan, printed by
/// the live harness beside each action line and never a gate.
const PERMISSION_PHRASES: [&str; 11] = [
    "engine set",
    "engine admits",
    "admitted by the engine",
    "engine's rules",
    "engine excludes",
    "excluded by the engine",
    "not allowed",
    "not permitted",
    "forbid",
    "prohibit",
    "outside the set",
];

/// The phrases no model-facing packet may carry once account economics leave
/// the intrinsic packets (fix list 3.2, `portfolio-v38`): the header's own
/// labels, the position's P/L vocabulary, and the sized-overlay form. Shared
/// by the offline isolation test and the live harness's prose scan. The bare
/// word "unrealized" is deliberately not listed: the distilled research can
/// legitimately carry an issuer's own "realized and unrealized capital losses"
/// (ARKF's fixture does), which is the issuer's condition, not the account's.
pub(crate) const ACCOUNT_ECONOMICS_PHRASES: [&str; 10] = [
    "cost basis:",
    "quantity:",
    "market value:",
    "unrealized gain",
    "unrealized loss",
    "p/l",
    "share-equivalents",
    "entry price",
    "purchase price",
    "/share cost",
];

/// Fix list 3.2 (ruled 2026-09-16): both intrinsic interpretation packets are
/// investment-only — the priced prompt on the six attempt-6 fixtures and the
/// role/risk prompt on a synthetic fund — so account economics cannot change
/// the packet and no economics phrase renders. The header, the direction-only
/// position line and the unsized overlay are the app-rendered routes this
/// pins; the model-authored prose the packets embed is scrubbed in the
/// fixtures and scanned live.
#[test]
fn attempt_6_interpretation_packets_carry_no_account_economics() {
    let repriced = |f: &Fixture| {
        let mut d = dossier_of(f, true);
        d.position.cost_basis *= 3.0;
        d.position.quantity *= 2.0;
        d.position.market_value *= 2.0;
        d
    };
    for f in fixtures() {
        let render = |d: &super::dossier::HoldingDossier| {
            interpretation_user_prompt(&InterpretationInput {
                input_delta: &[],
                dossier: d,
                prior_ledger: None,
                engine: &f.engine_output,
                distilled: &f.research_combined,
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            })
        };
        let taxable = render(&dossier_of(&f, true));
        assert_eq!(taxable, render(&dossier_of(&f, false)), "{}: tax posture", f.symbol);
        assert_eq!(taxable, render(&repriced(&f)), "{}: account economics", f.symbol);
        let lower = taxable.to_lowercase();
        for phrase in ACCOUNT_ECONOMICS_PHRASES {
            assert!(!lower.contains(phrase), "{}: {phrase} leaked: {taxable}", f.symbol);
        }
        assert!(
            taxable.starts_with(&format!("======== PART 1: INPUTS ========\nHOLDING\n{} (", f.symbol)),
            "{taxable}"
        );
        assert!(taxable.contains("This is the first analysis of this holding.\n"), "{taxable}");
        // Fix list 8.5 (portfolio-v39): a fixture carrying a computed debt / equity
        // stamps the equity source its live dossier would have, so the packet's
        // balance-sheet line never contradicts its own computed metrics.
        match (f.engine_output.metrics.debt_to_equity, f.equity_source) {
            // A fund has no statement series: its line names the market
            // metrics' cadence and the expense ratio's source instead.
            _ if f.is_fund => assert!(
                taxable.contains("\nFINANCIAL METRICS\nThe market metrics are daily; the expense ratio is the fund's published figure.\n"),
                "{}: {taxable}",
                f.symbol
            ),
            (Some(_), Some(src)) => assert!(
                taxable.contains(&format!("Balance-sheet metrics (debt / equity, P/B) are from {}.", src.label())),
                "{}: {taxable}",
                f.symbol
            ),
            (None, None) => assert!(
                taxable.contains("Balance-sheet metrics (debt / equity, P/B) have no balance sheet this run"),
                "{}: {taxable}",
                f.symbol
            ),
            (de, src) => panic!("{}: debt/equity {de:?} beside equity source {src:?}", f.symbol),
        }
    }
    // The role/risk branch on a synthetic fund (attempt 6 produced none).
    let financials = CompanyFinancials { symbol: "BND".into(), current_price: Some(72.0), ..Default::default() };
    let mut fund = pipeline::tests::dossier(AssetClass::Etf, financials);
    fund.position.symbol = "BND".into();
    let readout = super::fund::RoleRiskReadout::default();
    let render = |d: &super::dossier::HoldingDossier| {
        role_risk_user_prompt(&RoleRiskInput {
            input_delta: &[],
            dossier: d,
            prior_ledger: None,
            readout: &readout,
            ledger_eval: None,
            distilled: "No research findings.",
        })
    };
    let base = render(&fund);
    let mut moved = fund.clone();
    moved.position.cost_basis *= 3.0;
    moved.position.quantity *= 2.0;
    moved.position.market_value *= 2.0;
    moved.profile.tax_sensitive = !fund.profile.tax_sensitive;
    assert_eq!(base, render(&moved), "role/risk: account economics and tax posture");
    let lower = base.to_lowercase();
    for phrase in ACCOUNT_ECONOMICS_PHRASES {
        assert!(!lower.contains(phrase), "role/risk: {phrase} leaked: {base}");
    }
}

#[test]
fn the_fixed_set_is_labelled_reconstructed_and_carries_no_account_economics() {
    for (sym, json) in FIXTURES {
        assert!(json.contains("reconstructed, not replayed"), "{sym}");
        assert!(json.contains("synthetic"), "{sym}");
        // The synthetic position is exactly ten units, so a real quantity can
        // never have ridden in (the extracts' quantities were fractional).
        let f: Fixture = serde_json::from_str(json).unwrap();
        assert_eq!(f.synthetic_position.quantity, 10.0, "{sym}");
        // The model-authored strings that referenced the account's position —
        // the rationale's P/L sign, a self-assessment's implied entry, a
        // summary's per-share cost — are scrubbed, so no economics phrase
        // survives outside the origin label (review R1, 2026-09-16).
        let body: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut text = String::new();
        fn collect(v: &serde_json::Value, key: &str, out: &mut String) {
            match v {
                serde_json::Value::String(s) if key != "origin" => {
                    out.push_str(&s.to_lowercase());
                    out.push('\n');
                }
                serde_json::Value::Array(a) => a.iter().for_each(|v| collect(v, key, out)),
                serde_json::Value::Object(o) => o.iter().for_each(|(k, v)| collect(v, k, out)),
                _ => {}
            }
        }
        collect(&body, "", &mut text);
        for phrase in ["cost basis", "entry implied", "unrealized gain", "unrealized loss", "capital gains tax", "/share cost"] {
            assert!(!text.contains(phrase), "{sym}: '{phrase}' survives in a fixture string");
        }
    }
}

// ---- Synthetic fixtures (ruled 2026-09-16, C2): attempt 6 produced no role/risk
// holding and no second-run ledger, so the two paths this slice touches get
// synthetic offline cases, labelled as such; their live halves wait for real
// evidence.

/// Synthetic, not reconstructed: the role/risk branch's ledger under the same
/// 6g checks — a fund-computable core is kept, a stock-only series is
/// downgraded as uncomputable, and the authoring contract offers a fund no
/// stock-only series.
#[test]
fn synthetic_role_risk_fixture_scopes_the_contract_and_the_6g_checks_to_the_fund_surface() {
    let mut draft = pipeline::stub_ledger_draft(None, "BND", true);
    draft.falsifiers.push(FalsifierDraft {
        statement: "Net margin falls below 5%".into(),
        quant: Some(QuantCoreDraft {
            series: "net-margin".into(),
            comparator: "below".into(),
            threshold: 0.05,
            margin: 0.002,
        }),
        technology_class: false,
        tripped: false,
    });
    let (ledger, audit) = validate_ledger_rewrite_with_research(
        &draft,
        None,
        None,
        LedgerBranch::RoleRiskOnly,
        true,
        None,
        Some(72.0),
        &HashSet::new(),
        true,
        ContinuityStamps::NONE,
    );
    let expense = ledger
        .conditions
        .iter()
        .find(|c| c.statement.starts_with("Expense ratio"))
        .expect("the fund-flavored falsifier persists");
    assert!(expense.quant.is_some(), "{:?}", expense.downgraded_reason);
    let stock_series = ledger
        .conditions
        .iter()
        .find(|c| c.statement.starts_with("Net margin"))
        .expect("downgraded, never dropped");
    assert!(stock_series.quant.is_none());
    assert!(
        stock_series
            .downgraded_reason
            .as_deref()
            .is_some_and(|r| r.starts_with("series-uncomputable:")),
        "{:?}",
        stock_series.downgraded_reason
    );
    assert_eq!(audit.downgraded.len(), 1);
    let contract = pipeline::LedgerSeriesContract::build(true, None, None);
    assert!(contract.rows.iter().all(|r| r.series.computable_for(true)));
    assert!(!contract.rows.iter().any(|r| r.series == engine::LedgerSeries::NetMargin));
}

/// Synthetic, not reconstructed: a second-run carry over DIA's kept conditions.
/// A verbatim re-emission keeps its id and state; an edited threshold supersedes
/// into a fresh id with the link; a re-emission whose sentence now disagrees
/// with its core downgrades and loses its machine state rather than carrying it.
#[test]
fn synthetic_prior_ledger_continuity_fixture_carries_supersedes_and_downgrades() {
    let dia = fixtures().into_iter().find(|f| f.symbol == "DIA").unwrap();
    let first = validate_ledger_rewrite_with_research(
        &draft_of(&dia),
        None,
        None,
        LedgerBranch::Priced,
        true,
        None,
        Some(dia.spot),
        &HashSet::new(),
        true,
        stamps_of(&dia),
    )
    .0;
    let kept_ids: Vec<(String, String)> = first
        .conditions
        .iter()
        .filter(|c| c.quant.is_some())
        .map(|c| (c.statement.clone(), c.condition_id.clone()))
        .collect();
    assert_eq!(kept_ids.len(), 2, "DIA's two price cores");

    // Second run: the $575 falsifier re-emitted verbatim, the $648 trigger's
    // threshold edited, and the falsifier's sentence flipped against its core.
    let mut second = draft_of(&dia);
    for t in &mut second.triggers {
        if t.statement.starts_with("Price reaches +24%") {
            t.statement = "Price reaches +30% from current levels".into();
            t.quant.as_mut().unwrap().threshold = 680.0;
        }
    }
    let (ledger, audit) = validate_ledger_rewrite_with_research(
        &second,
        Some(&first),
        None,
        LedgerBranch::Priced,
        true,
        None,
        Some(dia.spot),
        &HashSet::new(),
        true,
        stamps_of(&dia),
    );
    let carried = ledger
        .conditions
        .iter()
        .find(|c| c.statement.starts_with("Price sustains above $575"))
        .unwrap();
    let (_, prior_id) = kept_ids
        .iter()
        .find(|(s, _)| s.starts_with("Price sustains above $575"))
        .unwrap();
    assert_eq!(&carried.condition_id, prior_id, "a verbatim core carries its id");
    assert!(carried.quant.is_some());
    let superseding = ledger
        .conditions
        .iter()
        .find(|c| c.statement.starts_with("Price reaches +30%"))
        .unwrap();
    let (_, old_id) = kept_ids
        .iter()
        .find(|(s, _)| s.starts_with("Price reaches +24%"))
        .unwrap();
    assert_ne!(&superseding.condition_id, old_id, "an edited core supersedes into a fresh id");
    assert_eq!(superseding.supersedes.as_deref(), Some(old_id.as_str()));
    assert_eq!(audit.superseded.len(), 1);

    // Third run: the falsifier's sentence now contradicts its core.
    let mut third = draft_of(&dia);
    for f in &mut third.falsifiers {
        if f.statement.starts_with("Price sustains above $575") {
            f.statement = "Price falls below $575, losing the base-case multiple".into();
        }
    }
    let (ledger, _) = validate_ledger_rewrite_with_research(
        &third,
        Some(&first),
        None,
        LedgerBranch::Priced,
        true,
        None,
        Some(dia.spot),
        &HashSet::new(),
        true,
        stamps_of(&dia),
    );
    let flipped = ledger
        .conditions
        .iter()
        .find(|c| c.statement.starts_with("Price falls below $575"))
        .unwrap();
    assert!(flipped.quant.is_none() && flipped.eval_state.is_none());
    assert!(
        flipped
            .downgraded_reason
            .as_deref()
            .is_some_and(|r| r.starts_with("comparator-mismatch:")),
        "{:?}",
        flipped.downgraded_reason
    );
    assert_ne!(&flipped.condition_id, prior_id, "a downgraded condition never inherits the machine core's id");
}

/// The §8.2 admission harness: the interpretation and action calls issued live
/// against the local daemon on the reconstructed packets, `N` repeats each, the
/// returned ledgers run through 6g, and a per-holding table printed for the
/// human read — accepted cores beside their sentences (does the core mean what
/// the sentence says?), downgrades by class (was a sound condition downgraded?),
/// the rung per repeat, and the rung under the tax and cost variants.
///
/// Requires the local Ollama daemon up with the configured roster present. Run:
///   `MARKET_SIGNAL_LOCAL_EVAL_REPEATS=3 cargo test fixed_evidence_live -- --ignored --nocapture`
/// `MARKET_SIGNAL_LOCAL_EVAL_SYMBOLS=TSLA,PGNY` narrows the set.
/// `MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR=<dir>` also captures every call's
/// thinking, fenced, into `<dir>/<stamp>-fixedevi/holding-<SYM>.txt` through
/// the same [`crate::thought_log::ThoughtLogSink`] the dev app's debug capture
/// uses, so the attempt-6 per-call segmentation reads the files unchanged
/// (re-think marker counts stay a diagnostic beside the table, never the gate
/// — fix list 8.4).
#[test]
#[ignore = "hits the live local daemon; set MARKET_SIGNAL_LOCAL_* and run with --nocapture"]
fn fixed_evidence_live() {
    use crate::config::AppConfig;
    use crate::local_model::{self, DaemonProbe, LocalModelClient};
    use crate::portfolio::pipeline::{HoldingAnalyst, LocalAnalyst};
    use crate::progress::{NoopReporter, ProgressReporter, RunContext};
    use crate::thought_log::ThoughtLogSink;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    let repeats: usize = std::env::var("MARKET_SIGNAL_LOCAL_EVAL_REPEATS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let only: Option<Vec<String>> = std::env::var("MARKET_SIGNAL_LOCAL_EVAL_SYMBOLS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_uppercase()).collect());
    let cfg = AppConfig::from_env();
    let endpoint = local_model::endpoint_from_config(&cfg).expect("MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT set");
    let roster = local_model::roster_from_config(&cfg);
    let thought_dir = std::env::var("MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let ctx = {
        let noop: Arc<dyn ProgressReporter> = Arc::new(NoopReporter);
        let reporter: Arc<dyn ProgressReporter> = match &thought_dir {
            Some(dir) => Arc::new(ThoughtLogSink::attach(noop, Path::new(dir), "fixed-evidence")),
            None => noop,
        };
        RunContext::new("fixed-evidence", reporter, Arc::new(AtomicBool::new(false)))
    };
    let client = LocalModelClient::new(&endpoint)
        .expect("build local client")
        .with_context(ctx.clone());
    match client.probe_daemon(&roster) {
        DaemonProbe::Reachable { missing } if missing.is_empty() => {}
        other => panic!("local daemon/roster not ready: {other:?}"),
    }
    let analyst = LocalAnalyst::new(client, roster.reasoner.clone(), roster.fast.clone());

    println!(
        "\n== fixed evidence, live (RECONSTRUCTED packets, not a replay of attempt 6) — {repeats} repeat(s) =="
    );
    if let Some(dir) = &thought_dir {
        println!("  thinking captured under {dir} (newest folder; one fenced holding-<SYM>.txt per holding)");
    }
    for f in fixtures() {
        if let Some(only) = &only {
            if !only.contains(&f.symbol) {
                continue;
            }
        }
        let VerdictDisposition::Priced(graded) = &f.disposition else { continue };
        let d = dossier_of(&f, true);
        // Bracket the holding as its own step so each call's fence lands in the
        // same `holding-<SYM>.txt` its thinking streams to (the sink files a
        // fence under the active step, a delta under its stream key).
        let step_key = crate::portfolio::holding_step_key(&f.symbol);
        ctx.step_started(step_key.clone(), format!("Analyze {}", f.symbol));
        println!("\n---- {} ({}; basis {:?}; spot {}) ----", f.symbol, if f.is_fund { "fund" } else { "stock" }, f.statement_basis, f.spot);
        println!(
            "  fidelity: engine numbers, options readings, distilled research and house view exact; the ledger contract's \
             market-data observations from the authoring close ({}), its filing-series observations \
             unavailable (statement rows not persisted); fund context, option overlay and pre-profit \
             overlay absent",
            f.authoring_close.date
        );

        // Interpretation → 6g, N repeats. The first completed interpretation
        // also feeds a fresh-verdict action call below (the §3 slice's Codex
        // plan review): the action reads the summary this interpretation wrote,
        // not the fixture's persisted one.
        let mut fresh: Option<(super::Interpretation, ThesisLedger)> = None;
        for r in 1..=repeats {
            let input = InterpretationInput {
                input_delta: &[],
                dossier: &d,
                prior_ledger: None,
                engine: &f.engine_output,
                distilled: &f.research_combined,
                ledger_eval: None,
                pre_profit: None,
                tech_pre_flag: None,
                narrative: None,
            };
            let started = std::time::Instant::now();
            let interp = match analyst.interpret(&input) {
                Ok(i) => i,
                Err(e) => {
                    println!("  interpretation #{r}: FAILED — {e:#}");
                    continue;
                }
            };
            // The §3 read: the prose fields beside the figures they should
            // explain, with two printed diagnostics — never gates — for the
            // human read: whether the target rationale names one of the
            // model's own figures (3.1) and whether any prose carries an
            // account-economics phrase (3.2).
            {
                let mt = &interp.model_price_targets;
                let engine_base = f.engine_output.price_targets.twelve_month.as_ref().map(|t| format!("{:.2}", t.base)).unwrap_or_else(|| "(gap)".into());
                let own: Vec<f64> = vec![mt.twelve_month.base, mt.twelve_month.bear, mt.twelve_month.bull, mt.one_month.base, mt.one_month.bear, mt.one_month.bull];
                let names_own = own.iter().any(|v| {
                    let whole = format!("{v:.0}");
                    interp.model_target_rationale.contains(&whole) || interp.model_target_rationale.contains(&format!("{v:.1}")) || interp.model_target_rationale.contains(&format!("{v:.2}"))
                });
                println!(
                    "    model 12-mo base {:.2} (bear {:.2} / bull {:.2}) vs engine base {engine_base}; rationale names an own figure: {}",
                    mt.twelve_month.base, mt.twelve_month.bear, mt.twelve_month.bull, if names_own { "yes" } else { "NO" }
                );
                for (label, text) in [
                    ("target rationale", interp.model_target_rationale.as_str()),
                    ("financial summary", interp.financial_summary.as_str()),
                    ("self-assessment", interp.self_assessment.as_str()),
                    ("what changed", interp.what_changed.as_str()),
                ] {
                    println!("    {label}: {}", text.replace('\n', " "));
                    prose_diagnostics(label, text);
                }
                println!("    what_changed_entries: {} (debut: app-written)", interp.what_changed_entries.len());
            }
            let (ledger, audit) = validate_ledger_rewrite_with_research(
                &interp.ledger, None, None, LedgerBranch::Priced, f.is_fund, None, Some(f.spot),
                &HashSet::new(), true, stamps_of(&f),
            );
            if fresh.is_none() {
                fresh = Some((interp.clone(), ledger.clone()));
            }
            // The prose the action packet forwards (Codex 2026-09-17, finding 3):
            // the validated ledger's thesis and scenario rows, printed so the
            // fresh-interpretation action rationale can be read against them.
            println!("    thesis: {}", ledger.current_thesis.replace('\n', " "));
            prose_diagnostics("thesis", &ledger.current_thesis);
            for s in &ledger.monitor {
                println!(
                    "    scenario {} ({:.0}%): {}",
                    s.scenario.as_str(), s.probability_pct, s.conditions.replace('\n', " ")
                );
                prose_diagnostics("scenario conditions", &s.conditions);
            }
            print_validated_ledger(&format!("interpretation #{r}"), started, &ledger, &audit);
        }

        // Action, N repeats, then the tax and cost variants on the fixed verdict,
        // then one call on the verdict assembled from the fresh interpretation
        // (the pipeline's own assembly), so clean interpretation prose is seen
        // reaching the rung.
        let engine_set = engine::feasible_actions(f.engine_output.grade, &f.engine_output.hurdle, None, false);
        let ledger = ledger_of(&f);
        let decide = |d: &super::dossier::HoldingDossier, subject: &super::GradedVerdict, ledger: &ThesisLedger, label: &str| {
            let started = std::time::Instant::now();
            match analyst.decide_action(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded: subject, engine: &f.engine_output, pre_profit: None, ledger },
                engine_set: &engine_set,
                changes: None,
                profile: &d.profile,
            }) {
                Ok(decision) => {
                    println!(
                        "  action {label}: {} ({:.0}s; set [{}]; run chose {}) — {}",
                        decision.action.as_kebab(), started.elapsed().as_secs_f64(),
                        engine_set.iter().map(Action::as_kebab).collect::<Vec<_>>().join(", "),
                        graded.action.as_kebab(), decision.rationale
                    );
                    // The 3.9 read's permission scan (a diagnostic, never a gate):
                    // a rationale that argues from what the computed read permits
                    // rather than from the evidence names itself here.
                    let lower = decision.rationale.to_lowercase();
                    if let Some(phrase) = PERMISSION_PHRASES.iter().find(|p| lower.contains(**p)) {
                        println!("    !! permission phrase in rationale: \"{phrase}\"");
                    }
                    prose_diagnostics("rationale", &decision.rationale);
                }
                Err(e) => println!("  action {label}: FAILED — {e:#}"),
            }
        };
        for r in 1..=repeats {
            decide(&d, graded, &ledger, &format!("#{r}"));
        }
        decide(&dossier_of(&f, false), graded, &ledger, "tax-exempt variant");
        let mut costly = dossier_of(&f, true);
        costly.position.cost_basis *= 3.0;
        decide(&costly, graded, &ledger, "cost-basis ×3 variant");
        if let Some((interp, fresh_ledger)) = fresh {
            let assembled = pipeline::graded_verdict_from_interpretation(
                &f.engine_output,
                d.options_signal.clone(),
                interp,
                graded.engine_view.clone(),
            );
            decide(&d, &assembled, &fresh_ledger, "fresh-interpretation verdict");
        }
        ctx.step_finished(step_key, "ok", None);
    }

    // The synthetic role/risk case (`portfolio-v42`, ruled 2026-09-17): one bond
    // fund with hand-written research, labelled synthetic — the branch's only
    // live exercise until a run supplies a real role/risk holding. Each repeat
    // is one interpretation validated on the role/risk branch and one action
    // call on that repeat's assembled verdict and validated ledger (ruling 11;
    // Codex 2026-09-17), so the action's variability is measured beside the
    // interpretation's.
    if only.as_ref().is_none_or(|o| o.iter().any(|s| s == "BND")) {
        let fx = synthetic_role_risk_fixture();
        let step_key = crate::portfolio::holding_step_key("BND");
        ctx.step_started(step_key.clone(), "Analyze BND (synthetic role/risk)");
        println!(
            "\n---- BND (SYNTHETIC role/risk bond fund; class {}; spot {}) ----",
            fx.readout.class_label,
            fx.dossier.financials.current_price.unwrap_or_default()
        );
        println!(
            "  fidelity: SYNTHETIC — the fund, its closes, its positioning line and its research \
             summary are hand-written; the house view is the fixed set's; the readout is the fund \
             engine's own on those inputs"
        );
        for r in 1..=repeats {
            let started = std::time::Instant::now();
            let interp = match analyst.interpret_role_risk(&synthetic_role_risk_input(&fx)) {
                Ok(i) => i,
                Err(e) => {
                    println!("  role/risk interpretation #{r}: FAILED — {e:#}");
                    continue;
                }
            };
            println!("    role read: {}", interp.role_summary.replace('\n', " "));
            prose_diagnostics("role read", &interp.role_summary);
            let (ledger, audit) = validate_ledger_rewrite_with_research(
                &interp.ledger, None, None, LedgerBranch::RoleRiskOnly, true, None,
                fx.dossier.financials.current_price, &HashSet::new(), true, ContinuityStamps::NONE,
            );
            println!("    thesis: {}", ledger.current_thesis.replace('\n', " "));
            prose_diagnostics("thesis", &ledger.current_thesis);
            for s in &ledger.monitor {
                println!(
                    "    scenario {} ({:.0}%): {}",
                    s.scenario.as_str(), s.probability_pct, s.conditions.replace('\n', " ")
                );
                prose_diagnostics("scenario conditions", &s.conditions);
            }
            print_validated_ledger(&format!("role/risk interpretation #{r}"), started, &ledger, &audit);

            // The action call on this repeat's verdict — the pipeline's own
            // assembly over the interpretation just validated.
            let rr = pipeline::role_risk_verdict_from_interpretation(&fx.readout, interp);
            let started = std::time::Instant::now();
            match analyst.decide_action(&ActionInput {
                dossier: &fx.dossier,
                subject: ActionSubject::RoleRisk { verdict: &rr, ledger: &ledger },
                engine_set: &super::ROLE_RISK_ACTIONS,
                changes: None,
                profile: &fx.dossier.profile,
            }) {
                Ok(decision) => {
                    println!(
                        "  action #{r} (role/risk, on interpretation #{r}): {} ({:.0}s; set [{}]) — {}",
                        decision.action.as_kebab(),
                        started.elapsed().as_secs_f64(),
                        super::ROLE_RISK_ACTIONS.iter().map(Action::as_kebab).collect::<Vec<_>>().join(", "),
                        decision.rationale
                    );
                    let lower = decision.rationale.to_lowercase();
                    if let Some(phrase) = PERMISSION_PHRASES.iter().find(|p| lower.contains(**p)) {
                        println!("    !! permission phrase in rationale: \"{phrase}\"");
                    }
                    prose_diagnostics("rationale", &decision.rationale);
                }
                Err(e) => println!("  action #{r} (role/risk): FAILED — {e:#}"),
            }
        }
        ctx.step_finished(step_key, "ok", None);
    }
}

/// The per-condition read of a validated ledger — the kept cores with the
/// margin beside its share of the level (fix list 3.14's anchoring watch), the
/// downgrades with their reasons, the authored qualitative conditions. A
/// diagnostic, never a gate.
fn print_validated_ledger(
    label: &str,
    started: std::time::Instant,
    ledger: &ThesisLedger,
    audit: &super::LedgerAudit,
) {
    let kept = ledger.conditions.iter().filter(|c| c.quant.is_some()).count();
    let qualitative = ledger.conditions.iter().filter(|c| c.quant.is_none() && c.downgraded_reason.is_none()).count();
    println!(
        "  {label}: {:.0}s — {} conditions: {kept} quantitative kept, {qualitative} authored qualitative, {} downgraded",
        started.elapsed().as_secs_f64(), ledger.conditions.len(), audit.downgraded.len()
    );
    for c in &ledger.conditions {
        match (&c.quant, &c.downgraded_reason) {
            (Some(q), _) => println!(
                "    KEPT   [{:?}{}] {} {} {} (margin {}, {}) <= \"{}\"",
                c.role, c.trigger_family.map(|f| format!(" {f:?}")).unwrap_or_default(),
                q.series.as_kebab(), q.comparator.as_kebab(), q.threshold, q.margin,
                if q.threshold != 0.0 {
                    format!("{:.0}% of level", q.margin / q.threshold.abs() * 100.0)
                } else {
                    "no ratio: zero level".to_string()
                },
                c.statement
            ),
            (None, Some(reason)) => println!("    DOWN   \"{}\" — {reason}", c.statement),
            (None, None) => println!("    QUAL   \"{}\"", c.statement),
        }
    }
}

// ---- The `portfolio-v40` structural pins: the two-part frame and the banned lexicon ----

/// The words the interpretation message must not contain (`portfolio-v40`,
/// ruled 2026-09-17): each is an app concept the model would reason about
/// instead of using. Matched as whole words, case-insensitive, over the
/// message's own sentences on the fixed set (the research and market text
/// are fixture-controlled here, so a hit is the app's).
pub(crate) const BANNED_LEXICON: [&str; 19] = [
    "arm", "arms", "engine", "seam", "the app", "deterministic", "this stage",
    "validator", "rejected", "downgrade", "downgraded", "baseline",
    "v1 mechanics", "clamp", "clamped", "degenerate", "inverse map", "trough", "targets-v",
];

/// The words of the engine's pre-v42 evidence-gap strings — the app explaining
/// its own routing — which no rendered packet may carry (ruled 2026-09-17; the
/// strings are data statements on every surface since `portfolio-v42`).
pub(crate) const GAP_ROUTING_WORDS: [&str; 6] =
    ["on-plan", "honestly", "degrade", "unsound", "this pipeline", "current data surface"];

/// Assert that a rendered packet carries none of [`GAP_ROUTING_WORDS`].
fn assert_no_routing_words(label: &str, text: &str) {
    let lower = text.to_lowercase();
    for w in GAP_ROUTING_WORDS {
        assert!(!lower.contains(w), "{label}: gap-routing word `{w}`\n{text}");
    }
}

pub(crate) fn banned_hits(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    BANNED_LEXICON
        .iter()
        .filter(|w| {
            let w = **w;
            lower.match_indices(w).any(|(i, _)| {
                let before = lower[..i].chars().next_back();
                let after = lower[i + w.len()..].chars().next();
                let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'));
                boundary(before) && boundary(after)
            })
        })
        .map(|w| w.to_string())
        .collect()
}

/// Every priced interpretation message on the fixed set is two marked parts in
/// order, Part 1 carrying the input sections and no instruction, Part 2 the
/// numbered task and the return shape — and neither part, nor the system
/// prompt, carries a banned word.
#[test]
fn attempt_6_interpretation_messages_are_two_parts_with_no_app_concept() {
    for f in fixtures() {
        let d = dossier_of(&f, true);
        let input = InterpretationInput {
            input_delta: &[], dossier: &d, prior_ledger: None, engine: &f.engine_output,
            distilled: &f.research_combined, ledger_eval: None, pre_profit: None,
            tech_pre_flag: None, narrative: None,
        };
        let user = interpretation_user_prompt(&input);
        let system = pipeline::interpretation_system_prompt(f.is_fund, true);
        let (part1, part2) = user
            .split_once("======== PART 2: TASK ========")
            .unwrap_or_else(|| panic!("{}: no Part 2 marker\n{user}", f.symbol));
        assert!(part1.starts_with("======== PART 1: INPUTS ========"), "{}: {part1}", f.symbol);
        for section in ["HOLDING\n", "FINANCIAL METRICS\n", "COMPUTED SCORES\n", "COMPUTED PRICE TARGETS (USD)\n", "OPTIONS ACTIVITY\n", "RESEARCH SUMMARY\n", "MARKET ANALYSIS\n", "PRIOR THESIS LEDGER\n"] {
            assert!(part1.contains(&format!("\n{section}")), "{}: Part 1 lacks {section}\n{part1}", f.symbol);
        }
        // The fund section renders where the fixture carries fund context; a
        // stock never has one.
        assert!(f.is_fund || !part1.contains("\nFUND\n"), "{}", f.symbol);
        // Part 1 instructs nothing: no "Return", no "you", no numbered task item.
        assert!(!part1.contains("Return "), "{}: Part 1 instructs\n{part1}", f.symbol);
        assert!(!part1.to_lowercase().contains("your "), "{}: Part 1 addresses the model\n{part1}", f.symbol);
        for item in ["1. financial_summary", "2. model_sub_scores", "3. model_price_targets", "4. horizon_outlook", "5. ledger", "6. conviction", "7. self_assessment", "RETURN SHAPE"] {
            assert!(part2.contains(item), "{}: Part 2 lacks {item}\n{part2}", f.symbol);
        }
        assert!(part2.contains("2 on a price of 100"), "{}", f.symbol);
        // The fund-scoped ledger item on the priced fund message too (ruled
        // 2026-09-17, `portfolio-v42`): the fund threshold example and the
        // driver clause on the three funds, the stock example and no driver
        // clause on the three stocks.
        assert_eq!(part2.contains("(\"above 0.75%\" on expense-ratio is 0.0075)"), f.is_fund, "{}", f.symbol);
        assert_eq!(
            part2.contains("for a fund, the exposure it supplies, its cost and its fidelity to its mandate"),
            f.is_fund,
            "{}",
            f.symbol
        );
        assert_eq!(part2.contains("(\"below 16%\" on gross-margin is 0.16)"), !f.is_fund, "{}", f.symbol);
        assert!(!user.contains("CAPITAL-EFFICIENCY") && !user.contains("hurdle"), "{}: the hurdle read leaked into the interpretation call\n{user}", f.symbol);
        assert!(!user.contains("at most 25%") && !user.contains("at most 50%"), "{}: the margin caps are shown\n{user}", f.symbol);
        assert!(!user.contains("Field alternatives") && !user.contains("Field notes"), "{}", f.symbol);
        for token in ["P75", "DGS10", "[targets-", "√t", "PR_base"] {
            assert!(!user.contains(token), "{}: internal target token {token}\n{user}", f.symbol);
        }
        for (label, text) in [("system", system.as_str()), ("user", user.as_str())] {
            let hits = banned_hits(text);
            assert!(hits.is_empty(), "{} {label} prompt carries {hits:?}\n{text}", f.symbol);
        }
        // The shape is JSON with exactly the declared keys.
        let shape_line = part2.lines().find(|l| l.starts_with('{')).expect("a shape line");
        let shape: serde_json::Value = serde_json::from_str(shape_line).expect("the shape parses");
        let mut keys: Vec<&str> = shape.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut declared = crate::portfolio::interpretation_keys(true);
        declared.sort_unstable();
        assert_eq!(keys, declared, "{}", f.symbol);
        // The system prompt names every declared key.
        for k in &declared {
            assert!(system.contains(k), "{}: system prompt does not name {k}\n{system}", f.symbol);
        }
    }
}

/// Every action message on the fixed set is two marked parts in order, Part 1
/// the input sections and no instruction, Part 2 the two items and the return
/// shape — and neither part, nor the system prompt, carries a banned word
/// (`portfolio-v41`). The analyst prose — the summary, the target rationale,
/// the thesis and the scenario conditions — is blanked for the lexicon scan
/// (ruled 2026-09-17, F3): the persisted v35 rationale legitimately says "the
/// engine's base case", and the pin tests the app's own sentences; the live
/// harness prints prose hits as a diagnostic.
#[test]
fn attempt_6_action_messages_are_two_parts_with_no_app_concept() {
    for f in fixtures() {
        let VerdictDisposition::Priced(graded) = &f.disposition else { panic!("{}: priced", f.symbol) };
        let d = dossier_of(&f, true);
        let engine_set = engine::feasible_actions(f.engine_output.grade, &f.engine_output.hurdle, None, false);
        let ledger = ledger_of(&f);
        let render = |graded: &super::GradedVerdict, ledger: &ThesisLedger| {
            action_user_prompt(&ActionInput {
                dossier: &d,
                subject: ActionSubject::Priced { graded, engine: &f.engine_output, pre_profit: None, ledger },
                engine_set: &engine_set,
                changes: None,
                profile: &d.profile,
            })
        };
        let user = render(graded, &ledger);
        let system = pipeline::action_system_prompt();
        let (part1, part2) = user
            .split_once("\n======== PART 2: TASK ========\n")
            .unwrap_or_else(|| panic!("{}: no Part 2 marker\n{user}", f.symbol));
        assert!(part1.starts_with(&format!("======== PART 1: INPUTS ========\nHOLDING\n{} (", f.symbol)), "{}: {part1}", f.symbol);
        for section in [
            "SCORES\n", "PRICE TARGETS (USD, with the move each implies from the current price)\n",
            "TARGET RATIONALE (analyst)\n", "CAPITAL EFFICIENCY\n", "CONVICTION AND OUTLOOK (analyst)\n",
            "FINANCIAL SUMMARY (analyst)\n", "THESIS (analyst)\n", "SCENARIOS (analyst)\n",
            "SUPPORTED ACTIONS (computed)\n", "INVESTOR PROFILE\n",
        ] {
            assert_eq!(part1.matches(&format!("\n{section}")).count(), 1, "{}: Part 1 lacks {section}\n{part1}", f.symbol);
        }
        // The thesis and the three scenario rows, as persisted.
        assert!(part1.contains(&format!("\nTHESIS (analyst)\n{}\n", f.ledger_prose.current_thesis)), "{}", f.symbol);
        assert_eq!(f.ledger_prose.monitor.len(), 3, "{}", f.symbol);
        for s in &f.ledger_prose.monitor {
            assert!(part1.contains(&format!(" ({:.0}%): {}\n", s.probability_pct, s.conditions)), "{}: {part1}", f.symbol);
        }
        // The hurdle as numbers: the three tested returns and the rate, no state word.
        let h = &f.engine_output.hurdle;
        assert!(part1.contains(&format!(
            "\nbear {:+.1}% / base {:+.1}% / bull {:+.1}%; hurdle {:.1}%.\n",
            h.tr_bear.unwrap() * 100.0, h.tr_base.unwrap() * 100.0, h.tr_bull.unwrap() * 100.0, h.hurdle_rate.unwrap() * 100.0
        )), "{}: {part1}", f.symbol);
        // Part 1 instructs nothing; Part 2 carries the two items and the shape.
        assert!(!part1.contains("Return "), "{}: Part 1 instructs\n{part1}", f.symbol);
        for item in ["1. action — one rung for this holding", "2. rationale — one sentence", "RETURN SHAPE (every value is a placeholder)"] {
            assert!(part2.contains(item), "{}: Part 2 lacks {item}\n{part2}", f.symbol);
        }
        for absent in [
            "ENGINE SET", "ENGINE ARM", "MODEL ARM", "THE VERDICT", "ACTION BASIS", "IMPLIED ",
            "TARGET PROVENANCE", "its own pick", "full ladder", "neither requires nor forbids",
            "indeterminate", "dead money", "PRIOR ACTION", "Move from PRIOR ACTION", "Keep the action firm",
        ] {
            assert!(!user.contains(absent), "{}: `{absent}`\n{user}", f.symbol);
        }
        // The lexicon over the app's own sentences: the analyst prose blanked.
        let mut blank = (**graded).clone();
        blank.financial_summary.clear();
        blank.model_target_rationale.clear();
        let mut blank_ledger = ledger.clone();
        blank_ledger.current_thesis.clear();
        for s in &mut blank_ledger.monitor {
            s.conditions.clear();
        }
        let blanked = render(&blank, &blank_ledger);
        for (label, text) in [("system", system.as_str()), ("user", blanked.as_str())] {
            let hits = banned_hits(text);
            assert!(hits.is_empty(), "{} {label} prompt carries {hits:?}\n{text}", f.symbol);
        }
        assert_no_routing_words(&f.symbol, &user);
        let (blank_part1, _) = blanked.split_once("\n======== PART 2: TASK ========\n").unwrap();
        assert!(!blank_part1.to_lowercase().contains("your "), "{}: Part 1 addresses the model\n{blank_part1}", f.symbol);
        // The shape is JSON with exactly the declared keys; the system prompt names them.
        let shape_line = part2.lines().find(|l| l.starts_with('{')).expect("a shape line");
        let shape: serde_json::Value = serde_json::from_str(shape_line).expect("the shape parses");
        let mut keys: Vec<&str> = shape.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut declared = crate::portfolio::ACTION_KEYS.to_vec();
        declared.sort_unstable();
        assert_eq!(keys, declared, "{}", f.symbol);
        for k in &declared {
            assert!(system.contains(k), "{}: system prompt does not name {k}\n{system}", f.symbol);
        }
    }
}

/// The role/risk message on the synthetic bond fund, debut and continuity, is
/// two marked parts in order with no app concept (`portfolio-v42`): Part 1 the
/// input sections in the draft's order and no instruction, Part 2 the numbered
/// task and the shape; each value once; the banned lexicon absent (the stub's
/// prose and the hand-written research carry none, so the whole message is
/// scanned); the system prompt the role line and every declared key.
#[test]
fn synthetic_role_risk_messages_are_two_parts_with_no_app_concept() {
    let fx = synthetic_role_risk_fixture();
    let debut_user = role_risk_user_prompt(&synthetic_role_risk_input(&fx));
    let debut_system = pipeline::role_risk_system_prompt(true);
    let (cont_system, cont_user) = synthetic_role_risk_continuity_messages();
    for (label, system, user, debut) in [
        ("debut", &debut_system, &debut_user, true),
        ("continuity", &cont_system, &cont_user, false),
    ] {
        let (part1, part2) = user
            .split_once("\n======== PART 2: TASK ========\n")
            .unwrap_or_else(|| panic!("{label}: no Part 2 marker\n{user}"));
        assert!(
            part1.starts_with(
                "======== PART 1: INPUTS ========\nHOLDING\nBND (VANGUARD TOTAL BOND MARKET ETF).\nPrice: $72.38 per share.\n"
            ),
            "{label}: {part1}"
        );
        let mut sections = vec![
            "CLASS\n", "EXPOSURE TILT\n", "UNDERLYING POSITIONING (CFTC weekly, as of 2026-09-09)\n",
            "RISK PROFILE\n", "EVIDENCE GAPS\n", "FINANCIAL METRICS\n", "RESEARCH SUMMARY\n",
            "MARKET ANALYSIS\n",
        ];
        if !debut {
            sections.extend([
                "PRIOR ANALYSIS (prior read 2026-09-03T14:00:00Z)\n",
                "CHANGES SINCE THE PRIOR ANALYSIS (each with an id)\n",
            ]);
        }
        sections.push("PRIOR THESIS LEDGER");
        if !debut {
            sections.push("CONDITION CROSSINGS THIS RUN\n");
        }
        let mut last = 0;
        for section in sections {
            let i = part1
                .find(&format!("\n{section}"))
                .unwrap_or_else(|| panic!("{label}: Part 1 lacks {section}\n{part1}"));
            assert!(i > last, "{label}: {section} out of order\n{part1}");
            last = i;
        }
        assert!(part1.contains("\nCLASS\nbond fund. Reported asset class: Fixed Income.\n"), "{label}: {part1}");
        assert!(
            part1.contains("\nEXPOSURE TILT\nThe fund's largest weights, by sector where reported and otherwise by country.\n- United States: 94.0%\n"),
            "{label}: {part1}"
        );
        assert!(part1.contains("\nEVIDENCE GAPS\nno duration, credit or yield-curve data for this fund\n"), "{label}: {part1}");
        assert!(part1.contains("\nRISK PROFILE\nAnnualized realized volatility: "), "{label}: {part1}");
        assert!(part1.contains("\nRESEARCH SUMMARY\nThe Vanguard Total Bond Market ETF tracks") || !debut, "{label}: {part1}");
        assert_eq!(part1.matches("0.0003 (0.03%/yr)").count(), 1, "{label}: the expense ratio renders once\n{part1}");
        assert_eq!(part1.matches("[return-volatility]").count(), 1, "{label}: the daily volatility renders once\n{part1}");
        assert!(!part1.contains("Return "), "{label}: Part 1 instructs\n{part1}");
        assert!(!part1.to_lowercase().contains("your "), "{label}: Part 1 addresses the model\n{part1}");
        let mut items = vec![
            "\n1. role_summary — a few sentences on the vehicle's mandate, the exposure it exists to supply, and the cost and risk of holding it, from CLASS, EXPOSURE TILT, RISK PROFILE, EVIDENCE GAPS, FINANCIAL METRICS and RESEARCH SUMMARY.\n",
            "\n2. ledger — ",
            "\nRETURN SHAPE (every value is a placeholder; an array holds as many items as apply)\n",
        ];
        if !debut {
            items.extend(["\n3. what_changed_entries — ", "\n   what_changed — one sentence summarizing those rows.\n"]);
        }
        for item in items {
            assert!(part2.contains(item), "{label}: Part 2 lacks {item}\n{part2}");
        }
        assert!(part2.contains("with family \"trim\" or \"sell\"") && !part2.contains("\"add\""), "{label}: {part2}");
        assert!(part2.contains("(\"above 0.75%\" on expense-ratio is 0.0075)"), "{label}: {part2}");
        assert!(part2.contains("for a fund, the exposure it supplies, its cost and its fidelity to its mandate"), "{label}: {part2}");
        assert!(part2.contains("drawing on MARKET ANALYSIS for the market setup"), "{label}: {part2}");
        for absent in [
            "CLASSIFICATION:", "STRUCTURAL FLAG", "EXPENSE RATIO (", "LEDGER OBSERVATION", "OBSERVABLE RISK",
            "DISTILLED RESEARCH", "MARKET SIGNAL", "HOUSE VIEW", "ACTION: author none", "CONTINUITY:",
            "WHAT_CHANGED_ENTRIES:", "METRICS AVAILABLE", "REWRITE THE THESIS LEDGER", "role/risk-only",
            "Field notes", "Field alternatives", "this pipeline", "this branch", "Keep the read firm",
            "in isolation", "on-plan", "honestly", "Position change since last run",
        ] {
            assert!(!user.contains(absent), "{label}: `{absent}`\n{user}");
        }
        for (l, text) in [("system", system.as_str()), ("user", user.as_str())] {
            let hits = banned_hits(text);
            assert!(hits.is_empty(), "{label} {l} prompt carries {hits:?}\n{text}");
        }
        assert_no_routing_words(&format!("BND {label}"), user);
        let shape_line = part2.lines().find(|l| l.starts_with('{')).expect("a shape line");
        let shape: serde_json::Value = serde_json::from_str(shape_line).expect("the shape parses");
        let mut keys: Vec<&str> = shape.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut declared = crate::portfolio::role_risk_keys(debut);
        declared.sort_unstable();
        assert_eq!(keys, declared, "{label}");
        assert!(
            system.starts_with(
                "You are an investment analyst producing an independent read of one fund holding for a portfolio review. Part 1 of the message gives the inputs. Part 2 states what to determine from them and the shape to return. You will return role_summary"
            ),
            "{label}: {system}"
        );
        for k in &declared {
            assert!(system.contains(k), "{label}: system prompt does not name {k}\n{system}");
        }
    }
    // The continuity specifics: the position sentence, the prior class and
    // role read as data, the changes with their ids.
    assert!(cont_user.contains("\nThe position is unchanged since the prior analysis.\n"), "{cont_user}");
    assert!(
        cont_user.contains("\nPRIOR ANALYSIS (prior read 2026-09-03T14:00:00Z)\n- prior class: bond fund.\n- prior role read: "),
        "{cont_user}"
    );
    assert!(cont_user.contains("\n[D1] "), "{cont_user}");
    assert!(!debut_user.contains("PRIOR ANALYSIS"), "{debut_user}");
}

/// The action message on the synthetic role/risk verdict is the v41 packet's
/// role/risk branch: two parts, the branch's sections, the reduced set as one
/// data line, no banned word over the app's own sentences (the analyst prose
/// blanked), the shape's keys (`portfolio-v42`, closing the v41 record's
/// offline-only limit on this branch).
#[test]
fn synthetic_role_risk_action_message_is_two_parts_with_no_app_concept() {
    let fx = synthetic_role_risk_fixture();
    let (rr, ledger) = synthetic_role_risk_verdict(&fx);
    let render = |verdict: &super::RoleRiskVerdict, ledger: &ThesisLedger| {
        action_user_prompt(&ActionInput {
            dossier: &fx.dossier,
            subject: ActionSubject::RoleRisk { verdict, ledger },
            engine_set: &super::ROLE_RISK_ACTIONS,
            changes: None,
            profile: &fx.dossier.profile,
        })
    };
    let user = render(&rr, &ledger);
    let system = pipeline::action_system_prompt();
    let (part1, part2) = user
        .split_once("\n======== PART 2: TASK ========\n")
        .unwrap_or_else(|| panic!("no Part 2 marker\n{user}"));
    assert!(part1.starts_with("======== PART 1: INPUTS ========\nHOLDING\nBND ("), "{part1}");
    for section in [
        "CLASS (computed)\n", "ROLE (analyst)\n", "EXPOSURE TILT (computed)\n", "RISK PROFILE (computed)\n",
        "EVIDENCE GAPS (computed)\n", "THESIS (analyst)\n", "SCENARIOS (analyst)\n",
        "SUPPORTED ACTIONS (computed)\n", "INVESTOR PROFILE\n",
    ] {
        assert_eq!(part1.matches(&format!("\n{section}")).count(), 1, "Part 1 lacks {section}\n{part1}");
    }
    assert!(part1.contains("\nEVIDENCE GAPS (computed)\nno duration, credit or yield-curve data for this fund\n"), "{part1}");
    assert!(part1.contains("\nSUPPORTED ACTIONS (computed)\nThe rungs the computed read supports on its own: sell-all, trim, hold.\n"), "{part1}");
    assert!(!part1.contains("Return "), "Part 1 instructs\n{part1}");
    for item in ["1. action — one rung for this holding", "2. rationale — one sentence", "RETURN SHAPE (every value is a placeholder)"] {
        assert!(part2.contains(item), "Part 2 lacks {item}\n{part2}");
    }
    assert!(!user.contains("CAPITAL EFFICIENCY") && !user.contains("PRICE TARGETS"), "{user}");
    let mut blank = rr.clone();
    blank.role_summary.clear();
    let mut blank_ledger = ledger.clone();
    blank_ledger.current_thesis.clear();
    for s in &mut blank_ledger.monitor {
        s.conditions.clear();
    }
    let blanked = render(&blank, &blank_ledger);
    for (label, text) in [("system", system.as_str()), ("user", blanked.as_str())] {
        let hits = banned_hits(text);
        assert!(hits.is_empty(), "{label} prompt carries {hits:?}\n{text}");
    }
    assert_no_routing_words("BND action", &user);
    let shape_line = part2.lines().find(|l| l.starts_with('{')).expect("a shape line");
    let shape: serde_json::Value = serde_json::from_str(shape_line).expect("the shape parses");
    let mut keys: Vec<&str> = shape.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut declared = crate::portfolio::ACTION_KEYS.to_vec();
    declared.sort_unstable();
    assert_eq!(keys, declared);
}

/// The research messages the harness renders (`portfolio-v43`): the gathering
/// and synthesis passes on TSLA's first stock topic and the synthetic fund's
/// exposure-profile topic, over hand-written leads, claims, conditions and
/// pages (`research::samples`) — the prompts' shape, never a run's research.
fn research_samples() -> Vec<super::research::samples::Sample> {
    let f = fixtures().into_iter().find(|f| f.symbol == "TSLA").expect("TSLA");
    let d = dossier_of(&f, true);
    let brief = pipeline::holding_header(&d);
    let agenda = super::research::build_agenda(&d, &super::research::AgendaTriggers::default());
    let fx = synthetic_role_risk_fixture();
    let fund_agenda =
        super::research::build_agenda(&fx.dossier, &super::research::AgendaTriggers::default());
    let exposure = fund_agenda
        .iter()
        .find(|t| t.key == "fund-exposure-profile")
        .expect("the retitled fund exposure topic (fix list 4.4)");
    let mut samples = super::research::samples::gathering_messages(
        &brief,
        &agenda[0],
        &super::research::samples::stock_leads(),
    );
    samples.extend(super::research::samples::synthesis_messages(&brief, &agenda[0]));
    let fund_brief = pipeline::holding_header(&fx.dossier);
    samples.extend(
        super::research::samples::gathering_messages(
            &fund_brief,
            exposure,
            &super::research::samples::fund_leads(),
        )
            .into_iter()
            .take(1)
            .map(|mut s| {
                s.label = format!("{} (SYNTHETIC fund, exposure profile)", s.label);
                s
            }),
    );
    samples
}

/// Every research message on the sample passes is two marked parts in order
/// with no app concept (`portfolio-v43`): Part 1 the input sections with the
/// dated holding header, the tier scale stated and no instruction; Part 2 the
/// task — on a gathering pass the per-reply bound and the stopping rule, on a
/// synthesis pass the numbered items and a shape whose keys carry the
/// follow-up on a topic pass and not on the disconfirming pass — and neither
/// part, nor the system prompt, carries a banned word or a routing word.
#[test]
fn research_messages_are_two_parts_with_no_app_concept() {
    let samples = research_samples();
    assert_eq!(samples.len(), 8, "four gathering, three synthesis, one fund gathering");
    for s in &samples {
        let (part1, part2) = s
            .user
            .split_once("======== PART 2: TASK ========")
            .unwrap_or_else(|| panic!("{}: no Part 2 marker\n{}", s.label, s.user));
        assert!(part1.starts_with("======== PART 1: INPUTS ========\nHOLDING\n"), "{}: {part1}", s.label);
        assert!(part1.contains("\nDate: 2026-09-16.\n"), "{}: no date line\n{part1}", s.label);
        assert!(part1.contains("\nTOPIC\n"), "{}: no TOPIC\n{part1}", s.label);
        assert!(part1.contains("0 is a primary source"), "{}: the tier scale is unstated\n{part1}", s.label);
        assert!(!part1.contains("recency"), "{}: recency rendered\n{part1}", s.label);
        assert!(
            !part1.contains("Return ") && !part1.to_lowercase().contains("your "),
            "{}: Part 1 instructs\n{part1}",
            s.label
        );
        for (label, text) in [("system", s.system.as_str()), ("user", s.user.as_str())] {
            let hits = banned_hits(text);
            assert!(hits.is_empty(), "{} {label} prompt carries {hits:?}\n{text}", s.label);
            assert_no_routing_words(&format!("{} {label}", s.label), text);
        }
        for word in [
            "orchestrator", "cached", "citable", "GATHER", "seeded_by", "S-id", "structured feeds",
            "input budget", "fetch cap", "turn cap", "treat coverage", "DISPROVE", "emerging thesis",
            "topic_answered", "material_forward_fact", "[seed-",
        ] {
            assert!(
                !s.user.contains(word) && !s.system.contains(word),
                "{}: {word} leaked\n{}",
                s.label,
                s.user
            );
        }
        if s.label.starts_with("gathering") {
            assert!(part2.contains("2. At most 8 tool calls in one reply."), "{}: {part2}", s.label);
            assert!(part2.contains("3. Stop when the questions are answered"), "{}: {part2}", s.label);
            assert!(part2.contains("a weak source lowers confidence"), "{}: {part2}", s.label);
        } else {
            let shape_line = part2.lines().find(|l| l.starts_with('{')).expect("a shape line");
            let shape: serde_json::Value = serde_json::from_str(shape_line).expect("the shape parses");
            let keys: Vec<&str> = shape.as_object().unwrap().keys().map(String::as_str).collect();
            let disconfirming = s.label.contains("disconfirming");
            assert_eq!(keys.contains(&"followup_question"), !disconfirming, "{}", s.label);
            assert_eq!(part2.contains("3. followup_question"), !disconfirming, "{}", s.label);
            assert!(part2.contains("1. findings") && part2.contains("2. claims"), "{}: {part2}", s.label);
            assert!(part1.contains("\nEVIDENCE\n") && part1.contains("=== S1: "), "{}: {part1}", s.label);
            assert_eq!(s.system.contains("follow-up proposal"), !disconfirming, "{}", s.label);
        }
        if s.label.contains("continuity") {
            assert!(part1.contains("\nSTANDING CONDITIONS\n") && part1.contains("\nPRIOR FINDINGS\n"), "{}", s.label);
            assert!(part2.contains("still holds and for what is newer"), "{}", s.label);
        }
        if s.label.contains("disconfirming") {
            assert!(part1.contains("\nCLAIMS SO FAR\n"), "{}", s.label);
        }
        if s.label.contains("incomplete") {
            assert!(part1.contains("\nSEARCHING\nSearching for this topic was incomplete: "), "{}", s.label);
            assert!(part2.contains(", SEARCHING included"), "{}", s.label);
        }
    }
    // The tool results carry no instruction either.
    for (label, text) in super::research::samples::tool_results() {
        assert!(
            !text.contains("Work with what you have") && !text.contains("contributes no evidence"),
            "{label}: {text}"
        );
        assert!(banned_hits(&text).is_empty(), "{label}: {text}");
    }
}

/// Every rendered fixed-set prompt to one Markdown file for a human read
/// (`MARKET_SIGNAL_LOCAL_EVAL_PROMPT_DUMP=<file>`): the system prompts once,
/// then per holding the interpretation message, the action message, and the
/// lines the tax and cost variants change, then the synthetic role/risk case,
/// then the research messages (`portfolio-v43`).
#[test]
#[ignore = "writes the rendered fixed-set prompts to MARKET_SIGNAL_LOCAL_EVAL_PROMPT_DUMP"]
fn fixed_evidence_prompt_dump() {
    let Ok(path) = std::env::var("MARKET_SIGNAL_LOCAL_EVAL_PROMPT_DUMP") else { return };
    let fence = |s: &str| format!("~~~~text\n{}\n~~~~\n\n", s.trim_end());
    let diff_lines = |base: &str, variant: &str| -> String {
        let b: Vec<&str> = base.lines().collect();
        let v: Vec<&str> = variant.lines().collect();
        let mut out = String::new();
        for l in &b { if !v.contains(l) { out.push_str(&format!("- {l}\n")); } }
        for l in &v { if !b.contains(l) { out.push_str(&format!("+ {l}\n")); } }
        if out.is_empty() { "(no line differs)\n".into() } else { out }
    };
    let mut out = String::new();
    out.push_str(&format!("# Fixed-set prompts as rendered — `{}`\n\n", super::PROMPT_VERSION));
    out.push_str("## 1. System prompts\n\n### 1a. Interpretation — stock, first analysis\n\n");
    out.push_str(&fence(&pipeline::interpretation_system_prompt(false, true)));
    out.push_str("### 1b. Interpretation — fund, first analysis\n\n");
    out.push_str(&fence(&pipeline::interpretation_system_prompt(true, true)));
    out.push_str("### 1c. Action\n\n");
    out.push_str(&fence(&pipeline::action_system_prompt()));
    let mut n = 2;
    for f in fixtures() {
        let VerdictDisposition::Priced(graded) = &f.disposition else { continue };
        let d = dossier_of(&f, true);
        let input = InterpretationInput {
            input_delta: &[], dossier: &d, prior_ledger: None, engine: &f.engine_output,
            distilled: &f.research_combined, ledger_eval: None, pre_profit: None,
            tech_pre_flag: None, narrative: None,
        };
        let interp = interpretation_user_prompt(&input);
        let engine_set = engine::feasible_actions(f.engine_output.grade, &f.engine_output.hurdle, None, false);
        let ledger = ledger_of(&f);
        macro_rules! action_input { ($dd:expr) => { ActionInput {
            dossier: $dd,
            subject: ActionSubject::Priced { graded, engine: &f.engine_output, pre_profit: None, ledger: &ledger },
            engine_set: &engine_set, changes: None, profile: &$dd.profile,
        } } }
        let list = action_user_prompt(&action_input!(&d));
        let dt = dossier_of(&f, false);
        let tax = action_user_prompt(&action_input!(&dt));
        let mut costly = dossier_of(&f, true);
        costly.position.cost_basis *= 3.0;
        let cost = action_user_prompt(&action_input!(&costly));
        out.push_str(&format!("## {n}. {} ({}; engine set [{}]; hurdle {:?})\n\n", f.symbol,
            if f.is_fund { "fund" } else { "stock" },
            engine_set.iter().map(Action::as_kebab).collect::<Vec<_>>().join(", "),
            f.engine_output.hurdle.state));
        out.push_str(&format!("### {n}a. Interpretation message ({} chars)\n\n", interp.len()));
        out.push_str(&fence(&interp));
        out.push_str(&format!("### {n}b. Action message ({} chars)\n\n", list.len()));
        out.push_str(&fence(&list));
        out.push_str(&format!("### {n}c. Tax-exempt variant: lines that differ from the action message\n\n"));
        out.push_str(&fence(&diff_lines(&list, &tax)));
        out.push_str(&format!("### {n}d. Cost-basis ×3 variant: lines that differ from the action message\n\n"));
        out.push_str(&fence(&diff_lines(&list, &cost)));
        n += 1;
    }
    // The synthetic role/risk case (`portfolio-v42`): both system prompts, the
    // debut message with the hand-written research, the continuity message the
    // pipeline renders on a stub second run, and the action message on the
    // stub's verdict.
    let fx = synthetic_role_risk_fixture();
    let debut = role_risk_user_prompt(&synthetic_role_risk_input(&fx));
    let (cont_system, cont) = synthetic_role_risk_continuity_messages();
    let (rr, ledger) = synthetic_role_risk_verdict(&fx);
    let action = action_user_prompt(&ActionInput {
        dossier: &fx.dossier,
        subject: ActionSubject::RoleRisk { verdict: &rr, ledger: &ledger },
        engine_set: &super::ROLE_RISK_ACTIONS,
        changes: None,
        profile: &fx.dossier.profile,
    });
    out.push_str(&format!(
        "## {n}. BND (SYNTHETIC role/risk bond fund — hand-written closes, positioning and research; the fixed set's house view; engine set [sell-all, trim, hold]; no hurdle)\n\n"
    ));
    out.push_str(&format!("### {n}a. Role/risk system prompt — first analysis\n\n"));
    out.push_str(&fence(&pipeline::role_risk_system_prompt(true)));
    out.push_str(&format!("### {n}b. Role/risk system prompt — continuity\n\n"));
    out.push_str(&fence(&cont_system));
    out.push_str(&format!("### {n}c. Role/risk message — first analysis ({} chars)\n\n", debut.len()));
    out.push_str(&fence(&debut));
    out.push_str(&format!("### {n}d. Role/risk message — continuity on the stub's first run ({} chars; the research summary is the offline stub's)\n\n", cont.len()));
    out.push_str(&fence(&cont));
    out.push_str(&format!("### {n}e. Action message on the stub's verdict ({} chars)\n\n", action.len()));
    out.push_str(&fence(&action));
    // The research messages (`portfolio-v43`): the gathering and synthesis
    // passes rendered on TSLA's first stock topic and the synthetic fund's
    // exposure-profile topic over hand-written leads, claims, conditions and
    // pages, then what a gathering turn gets back.
    let n = n + 1;
    out.push_str(&format!(
        "## {n}. Research messages (TSLA's competitive-position topic and the SYNTHETIC fund's exposure-profile topic; hand-written leads, claims, conditions and pages)\n\n"
    ));
    let mut letter = b'a';
    for s in research_samples() {
        out.push_str(&format!("### {n}{}. {} — system prompt\n\n", letter as char, s.label));
        out.push_str(&fence(&s.system));
        letter += 1;
        out.push_str(&format!("### {n}{}. {} — message ({} chars)\n\n", letter as char, s.label, s.user.len()));
        out.push_str(&fence(&s.user));
        letter += 1;
    }
    for (label, text) in super::research::samples::tool_results() {
        out.push_str(&format!("### {n}{}. Tool result — {label}\n\n", letter as char));
        out.push_str(&fence(&text));
        letter += 1;
    }
    std::fs::write(&path, out).expect("write prompt dump");
    println!("wrote {path}");
}
