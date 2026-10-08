//! The fixed evidence set from big-run attempt 6 — the verification substrate the
//! local-model-call slices are admitted against (fix list §8.1 / §8.2, ruled
//! 2026-09-15; `docs/verification/2026-09-16-ledger-conditions-and-action-packet.md`).
//!
//! Each fixture under `fixtures/attempt-6/` is **reconstructed, not replayed**:
//! reduced from a holding's persisted row (the verdict digest — its model arm
//! re-shaped by hand into a thesis document and a typed appendix — the engine
//! output assembled from the audit, the distilled research text) with the
//! position's economics replaced by synthetic values (ruled 2026-09-16, F8).
//! The offline tests render the thesis-document, appendix and action messages;
//! the `#[ignore]`d live harness issues the thesis conversation and the action
//! call against the local daemon on the same evidence and prints the
//! per-holding table the §8.2 admission read is made from.

use super::engine::{self, CompanyFinancials, EngineOutput};
use super::pipeline::{
    self, action_user_prompt, role_risk_user_prompt, tax_caveat, thesis_user_prompt, ActionInput,
    ActionSubject, HoldingAnalyst, RoleRiskInput, ThesisInput,
};
use super::{Action, AssetClass, PricedModelArm, StatementBasis, VerdictDisposition};
use crate::portfolio::dossier::HouseView;
use serde::Deserialize;

mod prompt_examples;

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
    /// close of the reconstructed financials.
    authoring_close: engine::DatedValue,
    synthetic_position: SyntheticPosition,
    /// The persisted verdict, its model arm re-shaped by hand: the thesis
    /// document composed from attempt 6's persisted prose (the thesis, the
    /// must-lines, the scenarios, the summary and the target rationale), the
    /// appendix from the persisted conviction with the twelve-month base as
    /// its one expected price (the three-month and three-year null — attempt
    /// 6 authored no such prices).
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

/// The shared rate anchors every harness input states under FETCHED VALUES.
fn rates() -> &'static engine::RateAnchors {
    pipeline::tests::rates_static()
}

/// The thesis-document input over a fixture on a first analysis: the engine
/// output and the distilled research as the run persisted them, no overlay,
/// no soft flags (the reconstructed financials carry no balance rows or
/// scores), no prior.
fn thesis_input<'a>(f: &'a Fixture, d: &'a super::dossier::HoldingDossier) -> ThesisInput<'a> {
    ThesisInput {
        dossier: d,
        engine: &f.engine_output,
        rates: rates(),
        analysis: &f.research_combined,
        pre_profit: None,
        soft_forensic: None,
        tech_pre_flag: None,
        narrative: None,
        prior_split: None,
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
        dossier: &fx.dossier,
        readout: &fx.readout,
        rates: rates(),
        analysis: fx.research,
        prior_split: None,
    }
}

/// The synthetic case's verdict off the stub's thesis document — the
/// pipeline's own assembly, so the action packet reads what a run would have
/// persisted.
fn synthetic_role_risk_verdict(fx: &SyntheticRoleRisk) -> super::RoleRiskVerdict {
    let document = pipeline::StubAnalyst
        .interpret_role_risk(&synthetic_role_risk_input(fx))
        .expect("the stub writes the document");
    pipeline::role_risk_verdict_from_model_arm(&fx.readout, document)
}

/// A stub that captures the role/risk message the pipeline renders — the
/// continuity variant only `analyze_holding` can build, since PRIOR THESIS is
/// the pipeline's read of the prior verdict.
#[derive(Default)]
struct RoleRiskCapture {
    system: std::cell::RefCell<Option<String>>,
    user: std::cell::RefCell<Option<String>>,
}

impl HoldingAnalyst for RoleRiskCapture {
    fn interpret(&self, input: &ThesisInput) -> anyhow::Result<PricedModelArm> {
        pipeline::StubAnalyst.interpret(input)
    }
    fn interpret_role_risk(&self, input: &RoleRiskInput) -> anyhow::Result<String> {
        *self.system.borrow_mut() = Some(pipeline::role_risk_system_prompt());
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
        let prompt = thesis_user_prompt(&thesis_input(&f, &d));
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
fn attempt_6_action_packets_carry_the_position_and_are_tax_invariant() {
    use super::pipeline::tests::without_position;
    for f in fixtures() {
        let VerdictDisposition::Priced(graded) = &f.disposition else {
            panic!("{}: every attempt-6 holding priced", f.symbol)
        };
        let engine_set = engine::feasible_actions(f.engine_output.grade, &f.engine_output.hurdle, None, false);
        let render = |d: &super::dossier::HoldingDossier| {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded, engine: &f.engine_output, pre_profit: None },
                engine_set: &engine_set,
                profile: &d.profile,
            })
        };
        let taxable = render(&dossier_of(&f, true));
        let exempt = render(&dossier_of(&f, false));
        assert_eq!(taxable, exempt, "{}: the tax posture must not change the packet", f.symbol);
        // The position's economics render once, under POSITION (`portfolio-v71`,
        // ruled 2026-10-08): the synthetic ten units and the synthetic P/L's
        // sign; a repriced position changes POSITION's lines and nothing else.
        assert_eq!(taxable.matches("\nPOSITION\n").count(), 1, "{}: {taxable}", f.symbol);
        assert!(taxable.contains("\nShares held: 10. Cost basis: $"), "{}: {taxable}", f.symbol);
        let p = &f.synthetic_position;
        let pl_word = if p.market_value > p.cost_basis { "Unrealized gain: $" } else { "Unrealized loss: $" };
        assert_eq!(taxable.matches(pl_word).count(), 1, "{}: {taxable}", f.symbol);
        let mut repriced = dossier_of(&f, true);
        repriced.position.cost_basis *= 3.0;
        repriced.position.quantity *= 2.0;
        repriced.position.market_value *= 2.0;
        let repriced = render(&repriced);
        assert_ne!(taxable, repriced, "{}", f.symbol);
        assert_eq!(without_position(&taxable), without_position(&repriced), "{}: account economics leaked past POSITION", f.symbol);
        // Case-insensitive outside POSITION: the app-rendered lines never carry
        // these, and the fixture's model-authored prose is scrubbed of them; the
        // tax row renders nowhere.
        let outside = without_position(&taxable).to_lowercase();
        for absent in ACCOUNT_ECONOMICS_PHRASES.iter().copied().chain(["- tax", "quantity:"]) {
            assert!(!outside.contains(absent), "{}: {absent} leaked: {taxable}", f.symbol);
        }
        assert!(!taxable.to_lowercase().contains("- tax"), "{}: {taxable}", f.symbol);
        // The set once as one data line (`portfolio-v41`); the sub-scores'
        // polarity gloss stays on the thesis message (ruled 2026-10-08).
        assert_eq!(taxable.matches("\nSUPPORTED ACTIONS\n").count(), 1, "{}", f.symbol);
        assert_eq!(taxable.matches("higher is better on every axis").count(), 0, "{}", f.symbol);
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
        let render = |d: &super::dossier::HoldingDossier| thesis_user_prompt(&thesis_input(&f, d));
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
        // No position line of any kind: the position's change reaches the
        // model at the action call alone (`docs/portfolio-analysis.md`
        // §Holdings change tracking).
        assert!(!taxable.contains("This is the first analysis of this holding."), "{taxable}");
        assert!(!taxable.contains("position is unchanged"), "{taxable}");
        // Fix list 8.5 (portfolio-v39): a fixture carrying a computed debt / equity
        // stamps the equity source its live dossier would have, so the packet's
        // balance-sheet line never contradicts its own computed metrics.
        match (f.engine_output.metrics.debt_to_equity, f.equity_source) {
            // A fund has no statement series: its line names the market
            // metrics' cadence and the expense ratio's source instead.
            _ if f.is_fund => assert!(
                taxable.contains("\nMETRICS\nThe market metrics are daily; the expense ratio is the fund's published figure.\n"),
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
            dossier: d,
            readout: &readout,
            rates: rates(),
            analysis: "No research findings.",
            prior_split: None,
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

/// The §8.2 admission harness: the thesis-document conversation and the action
/// call issued live against the local daemon on the reconstructed packets, `N`
/// repeats each, and a per-holding table printed for the human read — the
/// document's opening and its appendix beside the computed bands, the rung
/// per repeat, and the rung under the tax and cost variants.
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
    let price = |v: Option<f64>| v.map(|p| format!("{p:.2}")).unwrap_or_else(|| "none".into());
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
            "  fidelity: engine numbers, options readings, distilled research and house view exact; \
             the statement rows, the fund context, the option overlay, the pre-profit overlay and \
             the soft forensic flags absent (not persisted)"
        );

        // The thesis conversation, N repeats. The first completed one also
        // feeds a fresh-verdict action call below (the §3 slice's Codex plan
        // review): the action reads the document this conversation wrote, not
        // the fixture's persisted one.
        let mut fresh: Option<PricedModelArm> = None;
        let engine_base = f.engine_output.price_targets.twelve_month.as_ref().map(|t| format!("{:.2}", t.base)).unwrap_or_else(|| "(gap)".into());
        for r in 1..=repeats {
            let started = std::time::Instant::now();
            let arm = match analyst.interpret(&thesis_input(&f, &d)) {
                Ok(a) => a,
                Err(e) => {
                    println!("  thesis #{r}: FAILED — {e:#}");
                    continue;
                }
            };
            let a = &arm.appendix;
            println!(
                "  thesis #{r}: {:.0}s — {} words; appendix conviction {}, expected 3m {} / 12m {} / 3y {} vs engine 12-mo base {engine_base}",
                started.elapsed().as_secs_f64(),
                arm.thesis_document.split_whitespace().count(),
                a.conviction.map(|c| c.as_str()).unwrap_or("none"),
                price(a.expected_price_3m),
                price(a.expected_price_12m),
                price(a.expected_price_3y),
            );
            // The document's opening and closing paragraphs, with the prose
            // diagnostics — never a gate.
            let paragraphs: Vec<&str> = arm.thesis_document.split("\n\n").collect();
            if let Some(first) = paragraphs.first() {
                println!("    opens: {}", first.replace('\n', " "));
            }
            if paragraphs.len() > 1 {
                println!("    closes: {}", paragraphs[paragraphs.len() - 1].replace('\n', " "));
            }
            prose_diagnostics("thesis document", &arm.thesis_document);
            if fresh.is_none() {
                fresh = Some(arm);
            }
        }

        // Action, N repeats, then the tax and cost variants on the fixed verdict,
        // then one call on the verdict assembled from the fresh document (the
        // pipeline's own assembly), so clean prose is seen reaching the rung.
        let engine_set = engine::feasible_actions(f.engine_output.grade, &f.engine_output.hurdle, None, false);
        let decide = |d: &super::dossier::HoldingDossier, subject: &super::GradedVerdict, label: &str| {
            let started = std::time::Instant::now();
            match analyst.decide_action(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded: subject, engine: &f.engine_output, pre_profit: None },
                engine_set: &engine_set,
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
            decide(&d, graded, &format!("#{r}"));
        }
        decide(&dossier_of(&f, false), graded, "tax-exempt variant");
        let mut costly = dossier_of(&f, true);
        costly.position.cost_basis *= 3.0;
        decide(&costly, graded, "cost-basis ×3 variant");
        if let Some(arm) = fresh {
            let assembled = pipeline::graded_verdict_from_model_arm(
                &f.engine_output,
                d.options_signal.clone(),
                arm,
                graded.engine_rung,
                graded.authored_band_relation,
            );
            decide(&d, &assembled, "fresh-document verdict");
        }
        ctx.step_finished(step_key, "ok", None);
    }

    // The synthetic role/risk case (`portfolio-v42`, ruled 2026-09-17): one bond
    // fund with hand-written research, labelled synthetic — the branch's only
    // live exercise until a run supplies a real role/risk holding. Each repeat
    // is one thesis document and one action call on that repeat's assembled
    // verdict (ruling 11; Codex 2026-09-17), so the action's variability is
    // measured beside the document's.
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
            let document = match analyst.interpret_role_risk(&synthetic_role_risk_input(&fx)) {
                Ok(doc) => doc,
                Err(e) => {
                    println!("  role/risk thesis #{r}: FAILED — {e:#}");
                    continue;
                }
            };
            println!(
                "  role/risk thesis #{r}: {:.0}s — {} words",
                started.elapsed().as_secs_f64(),
                document.split_whitespace().count()
            );
            if let Some(first) = document.split("\n\n").next() {
                println!("    opens: {}", first.replace('\n', " "));
            }
            prose_diagnostics("thesis document", &document);

            // The action call on this repeat's verdict — the pipeline's own
            // assembly over the document just written.
            let rr = pipeline::role_risk_verdict_from_model_arm(&fx.readout, document);
            let started = std::time::Instant::now();
            match analyst.decide_action(&ActionInput {
                dossier: &fx.dossier,
                subject: ActionSubject::RoleRisk { verdict: &rr },
                engine_set: &super::ROLE_RISK_ACTIONS,
                profile: &fx.dossier.profile,
            }) {
                Ok(decision) => {
                    println!(
                        "  action #{r} (role/risk, on thesis #{r}): {} ({:.0}s; set [{}]) — {}",
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
/// Every Part 1 heading a thesis-document message's Part 2 names renders in
/// Part 1 — no item points at absent content.
fn assert_part2_headings_render(label: &str, part1: &str, part2: &str) {
    for heading in [
        "FETCHED VALUES", "COMPUTED", "MARKET ANALYSIS", "ANALYSIS", "PRIOR THESIS", "CLASS",
        "EXPOSURE TILT", "RISK PROFILE", "EVIDENCE GAPS", "SOFT FORENSIC FLAGS",
    ] {
        if part2.contains(heading) {
            assert!(
                part1.contains(&format!("\n{heading}\n")) || part1.contains(&format!("\n{heading} (")),
                "{label}: Part 2 names {heading}, which Part 1 does not render\n{part1}"
            );
        }
    }
}

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

/// Every thesis-document message on the fixed set is two marked parts in
/// order, Part 1 carrying the input sections in the doc's order and no
/// instruction, Part 2 the document's items in order and the length band
/// (no JSON shape — the document is free prose) — and neither part, nor the
/// system prompt, nor the appendix message, carries a banned word.
#[test]
fn attempt_6_thesis_messages_are_two_parts_with_no_app_concept() {
    for f in fixtures() {
        let d = dossier_of(&f, true);
        let input = thesis_input(&f, &d);
        let user = thesis_user_prompt(&input);
        let system = pipeline::thesis_system_prompt(f.is_fund);
        let (part1, part2) = user
            .split_once("======== PART 2: TASK ========")
            .unwrap_or_else(|| panic!("{}: no Part 2 marker\n{user}", f.symbol));
        assert!(part1.starts_with("======== PART 1: INPUTS ========"), "{}: {part1}", f.symbol);
        // The sections in page order: the header, the fetched data, the computed
        // reads under one heading, the market analysis, this run's analysis.
        let mut last = 0;
        for section in [
            "HOLDING\n", "FETCHED VALUES\n", "COMPUTED\n", "METRICS\n", "SCORES\n",
            "PRICE BANDS (USD)\n", "CAPITAL EFFICIENCY\n", "OPTIONS ACTIVITY\n", "MARKET ANALYSIS\n",
            "ANALYSIS\n",
        ] {
            let i = part1
                .find(&format!("\n{section}"))
                .unwrap_or_else(|| panic!("{}: Part 1 lacks {section}\n{part1}", f.symbol));
            assert!(i > last, "{}: {section} out of order\n{part1}", f.symbol);
            last = i;
        }
        // A debut carries no PRIOR THESIS and no continuity clause.
        assert!(!part1.contains("PRIOR THESIS"), "{}: {part1}", f.symbol);
        assert!(!part2.contains("PRIOR THESIS"), "{}: {part2}", f.symbol);
        // The scores line carries the grade and the imputed disclosure as data.
        let scores = part1.split("\nSCORES\n").nth(1).unwrap().split("\nPRICE BANDS").next().unwrap();
        assert!(scores.contains(&format!("Grade {}.", f.engine_output.grade.as_str())), "{}: {scores}", f.symbol);
        assert_eq!(scores.contains("One score is imputed."), f.engine_output.low_confidence_grade);
        // The three bands with their method clauses, the three-year leg as the
        // extrapolation it is, and the capital-efficiency read with its state.
        assert!(part1.contains("- three-month: bear ") && part1.contains("prorated to three months"), "{}", f.symbol);
        assert!(part1.contains("- three-year: bear ") && part1.contains("extrapolation"), "{}", f.symbol);
        assert!(part1.contains("; hurdle ") && part1.contains("; read: "), "{}: {part1}", f.symbol);
        // FETCHED VALUES states the quote, the fetched closes and the Treasury
        // prints as data; the reconstructed financials carry no statement rows.
        assert!(part1.contains(&format!("Quote: {:.2} per share (the live print, undated).\n", f.spot)), "{}: {part1}", f.symbol);
        assert!(part1.contains("Treasury yields (FRED): 10-year "), "{}: {part1}", f.symbol);
        // The fund section renders where the fixture carries fund context; a
        // stock never has one.
        assert!(f.is_fund || !part1.contains("\nFUND\n"), "{}", f.symbol);
        // Part 1 instructs nothing: no "Return", no "you", no numbered task item.
        assert!(!part1.contains("Return "), "{}: Part 1 instructs\n{part1}", f.symbol);
        assert!(!part1.to_lowercase().contains("your "), "{}: Part 1 addresses the model\n{part1}", f.symbol);
        // Part 2: the document's items in order, each naming a Part 1 section,
        // the length band, and no JSON shape.
        let mut last = 0;
        for item in [
            "\n1. The thesis — ", "\n2. The key drivers — ", "\n3. The bear, base and bull scenarios — ",
            "\n4. The falsifiers — ", "\n5. The expected share price at three months, twelve months and three years, ",
            "\n6. A summary paragraph — ",
        ] {
            let i = part2.find(item).unwrap_or_else(|| panic!("{}: Part 2 lacks {item}\n{part2}", f.symbol));
            assert!(i > last, "{}: {item} out of order", f.symbol);
            last = i;
        }
        assert!(part2.contains("The document runs 900 to 1,800 words."), "{}: {part2}", f.symbol);
        assert!(part2.contains("from FETCHED VALUES, COMPUTED, ANALYSIS and MARKET ANALYSIS"), "{}: {part2}", f.symbol);
        assert!(part2.contains("the price bands under COMPUTED are evidence, not bounds"), "{}: {part2}", f.symbol);
        assert!(!part2.contains("RETURN SHAPE") && !part2.contains("JSON object"), "{}: {part2}", f.symbol);
        assert!(!part2.lines().any(|l| l.starts_with('{')), "{}: a shape line\n{part2}", f.symbol);
        assert_part2_headings_render(&f.symbol, part1, part2);
        // The investor profile is absent — the intrinsic verdict is of no investor.
        assert!(!user.contains("INVESTOR PROFILE"), "{}", f.symbol);
        for token in ["P75", "DGS10", "[targets-", "√t", "PR_base"] {
            assert!(!user.contains(token), "{}: internal target token {token}\n{user}", f.symbol);
        }
        for (label, text) in [("system", system.as_str()), ("user", user.as_str())] {
            let hits = banned_hits(text);
            assert!(hits.is_empty(), "{} {label} prompt carries {hits:?}\n{text}", f.symbol);
        }
        assert_no_routing_words(&f.symbol, &user);
        // The request pair: the thesis request carries no grammar under
        // thinking; the appendix request continues the conversation — the same
        // system and user messages, the document as the assistant's turn, the
        // transcription ask — under the nullable grammar with thinking off.
        let thesis = pipeline::thesis_request("r", &input);
        assert!(thesis.format_schema.is_none() && thesis.think == Some(true), "{}", f.symbol);
        assert_eq!(thesis.messages.len(), 2);
        let appendix = pipeline::appendix_request("r", &input, "[stub: the document]");
        assert_eq!(appendix.think, Some(false));
        assert_eq!(appendix.format_schema.as_ref(), Some(&crate::portfolio::appendix_schema()));
        assert_eq!(appendix.messages.len(), 4);
        assert_eq!(appendix.messages[0].content, thesis.messages[0].content);
        assert_eq!(appendix.messages[1].content, thesis.messages[1].content);
        assert_eq!((appendix.messages[2].role.as_str(), appendix.messages[2].content.as_str()), ("assistant", "[stub: the document]"));
        let ask = &appendix.messages[3].content;
        assert!(ask.contains("A field is null where the document states no value."), "{ask}");
        assert!(ask.ends_with(&format!("RETURN SHAPE (every value is a placeholder)\n{}\n", crate::portfolio::appendix_return_shape())), "{ask}");
        assert!(banned_hits(ask).is_empty(), "{ask}");
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
        let render = |graded: &super::GradedVerdict| {
            action_user_prompt(&ActionInput {
                dossier: &d,
                subject: ActionSubject::Priced { graded, engine: &f.engine_output, pre_profit: None },
                engine_set: &engine_set,
                profile: &d.profile,
            })
        };
        let user = render(graded);
        let system = pipeline::action_system_prompt();
        let (part1, part2) = user
            .split_once("\n======== PART 2: TASK ========\n")
            .unwrap_or_else(|| panic!("{}: no Part 2 marker\n{user}", f.symbol));
        assert!(part1.starts_with(&format!("======== PART 1: INPUTS ========\nHOLDING\n{} (", f.symbol)), "{}: {part1}", f.symbol);
        // The sections in the docs' order (`portfolio-v71`): POSITION, VERDICT,
        // one COMPUTED heading with its labelled sub-blocks, SUPPORTED ACTIONS
        // and the profile, each once.
        let mut last = 0;
        for section in [
            "POSITION\n", "VERDICT\n", "COMPUTED\n", "COMPUTED ACTION\n", "GRADE\n",
            "PRICE BANDS (USD, with the move each implies from the current price; the analyst's expected price from VERDICT beside each)\n",
            "CAPITAL EFFICIENCY\n", "SUPPORTED ACTIONS\n", "INVESTOR PROFILE\n",
        ] {
            let key = format!("\n{section}");
            assert_eq!(part1.matches(&key).count(), 1, "{}: Part 1 lacks {section}\n{part1}", f.symbol);
            let at = part1.find(&key).unwrap();
            assert!(at > last, "{}: {section} out of order\n{part1}", f.symbol);
            last = at;
        }
        assert!(part1.contains(&format!("valuation and risk scores.\n{}", graded.grade.as_str())), "{}: {part1}", f.symbol);
        assert!(part1.contains("prorated to three months"));
        assert!(part1.contains("- three-year: bear ") && part1.contains("extrapolation"), "{}", f.symbol);
        assert!(part2.contains("Name the returns you weighed by their values; do not describe them by their relation to another figure."));
        // VERDICT: the gloss, then the appendix's conviction and prices — the
        // fixture's one expected price at twelve months, none at the other
        // horizons — then the thesis document verbatim; the same price rides
        // the twelve-month band line and none the other two (ruled 2026-10-08).
        let a = &graded.appendix;
        let move_12 = (a.expected_price_12m.unwrap() / f.spot - 1.0) * 100.0;
        assert!(
            part1.contains(&format!(
                "\nVERDICT\nAn analyst's read of the holding's data and research: the conviction, the expected share price at each horizon (USD, with the move each implies from the current price) and the thesis document.\nConviction: {}. Expected share price: three-month none, twelve-month {:.2} ({:+.1}%), three-year none.\nThesis document:\n{}",
                a.conviction.unwrap().as_str(),
                a.expected_price_12m.unwrap(),
                move_12,
                graded.thesis_document
            )),
            "{}: {part1}",
            f.symbol
        );
        assert!(
            part1.contains(&format!("; analyst {:.2} ({:+.1}%). Method: ", a.expected_price_12m.unwrap(), move_12)),
            "{}: {part1}",
            f.symbol
        );
        assert_eq!(part1.matches("; analyst none. Method: ").count(), 2, "{}: {part1}", f.symbol);
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
            "TARGET RATIONALE", "CONVICTION AND OUTLOOK", "FINANCIAL SUMMARY", "THESIS (analyst)",
            "SCENARIOS (analyst)", "PRIOR ANALYSIS", "CHANGES SINCE",
            "(computed)", "(analyst)", "Two reads of this holding", "higher is better on every axis",
        ] {
            assert!(!user.contains(absent), "{}: `{absent}`\n{user}", f.symbol);
        }
        // The lexicon over the app's own sentences: the analyst prose — the
        // thesis document — blanked.
        let mut blank = (**graded).clone();
        blank.thesis_document.clear();
        let blanked = render(&blank);
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

/// The role/risk thesis-document message on the synthetic bond fund, debut and
/// continuity, is two marked parts in order with no app concept: Part 1 the
/// input sections in the doc's order and no instruction, Part 2 the document's
/// items with no prices and no conviction and no JSON shape; each value once;
/// the banned lexicon absent (the stub's prose and the hand-written research
/// carry none, so the whole message is scanned); the system prompt the role
/// line and the two-part frame.
#[test]
fn synthetic_role_risk_messages_are_two_parts_with_no_app_concept() {
    let fx = synthetic_role_risk_fixture();
    let debut_user = role_risk_user_prompt(&synthetic_role_risk_input(&fx));
    let debut_system = pipeline::role_risk_system_prompt();
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
            "FETCHED VALUES\n", "CLASS\n", "EXPOSURE TILT\n", "UNDERLYING POSITIONING (CFTC weekly, as of 2026-09-09)\n",
            "RISK PROFILE\n", "EVIDENCE GAPS\n", "COMPUTED\n", "MARKET ANALYSIS\n", "ANALYSIS\n",
        ];
        if !debut {
            sections.push("PRIOR THESIS (written 2026-09-03)\n");
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
        assert!(part1.contains("\nANALYSIS\nThe Vanguard Total Bond Market ETF tracks") || !debut, "{label}: {part1}");
        // The fund's reported lines under FETCHED VALUES, as the provider
        // returns them.
        assert!(part1.contains("Fund: asset class Fixed Income; expense ratio 0.0003 (0.03%/yr); assets under management 340.0B; NAV 72.41.\n"), "{label}: {part1}");
        assert!(part1.contains("Country weights: United States 94.0%, Supranational 2.0%, Canada 1.0%.\n"), "{label}: {part1}");
        assert_eq!(part1.matches("daily realized return volatility: ").count(), 1, "{label}: the daily volatility renders once\n{part1}");
        assert!(!part1.contains("Return "), "{label}: Part 1 instructs\n{part1}");
        assert!(!part1.to_lowercase().contains("your "), "{label}: Part 1 addresses the model\n{part1}");
        // No position line on either run: the delta reaches the action call alone.
        assert!(!part1.contains("position is unchanged") && !part1.contains("first analysis of this holding"), "{label}: {part1}");
        let items = [
            "\n1. The role — the mandate and the exposure the vehicle exists to supply, and the cost and risk of holding it, from CLASS, EXPOSURE TILT, RISK PROFILE, EVIDENCE GAPS, FETCHED VALUES, COMPUTED, ANALYSIS and MARKET ANALYSIS.\n",
            "\n2. The risks — ",
            "\n3. The triggers for trimming or selling — each a concrete measure, a level and a period.\n",
            "\n4. A summary paragraph — the read as a whole",
            "\nThe document states no expected price and no conviction. It runs 900 to 1,800 words.\n",
        ];
        for item in items {
            assert!(part2.contains(item), "{label}: Part 2 lacks {item}\n{part2}");
        }
        assert_eq!(part2.contains(", and what changed since the prior analysis, drawing on PRIOR THESIS"), !debut, "{label}: {part2}");
        assert!(!part2.contains("RETURN SHAPE") && !part2.lines().any(|l| l.starts_with('{')), "{label}: {part2}");
        assert_part2_headings_render(label, part1, part2);
        for absent in [
            "CLASSIFICATION:", "STRUCTURAL FLAG", "EXPENSE RATIO (", "LEDGER", "OBSERVABLE RISK",
            "DISTILLED RESEARCH", "MARKET SIGNAL", "HOUSE VIEW", "ACTION: author none", "CONTINUITY:",
            "WHAT_CHANGED", "METRICS AVAILABLE", "role/risk-only", "RESEARCH SUMMARY", "FINANCIAL METRICS",
            "Field notes", "Field alternatives", "this pipeline", "this branch", "Keep the read firm",
            "in isolation", "on-plan", "honestly", "Position change since last run", "PRIOR ANALYSIS",
        ] {
            assert!(!user.contains(absent), "{label}: `{absent}`\n{user}");
        }
        for (l, text) in [("system", system.as_str()), ("user", user.as_str())] {
            let hits = banned_hits(text);
            assert!(hits.is_empty(), "{label} {l} prompt carries {hits:?}\n{text}");
        }
        assert_no_routing_words(&format!("BND {label}"), user);
        assert_eq!(
            system,
            "You are an investment analyst writing the thesis document for one fund holding in a \
             portfolio review. Part 1 of the message gives the inputs. Part 2 says what the document \
             covers, in order, and how to return it."
        );
    }
    // The continuity specifics: the prior document verbatim under its date,
    // the stub's role read opening it; no split line without a split.
    assert!(
        cont_user.contains("\nPRIOR THESIS (written 2026-09-03)\nRole: bond fund supplying United States exposure; held for its portfolio role.\n"),
        "{cont_user}"
    );
    assert!(!cont_user.contains("A share split since this document was written"), "{cont_user}");
    assert!(!debut_user.contains("PRIOR THESIS"), "{debut_user}");
}

/// The action message on the synthetic role/risk verdict is the v41 packet's
/// role/risk branch: two parts, the branch's sections, the reduced set as one
/// data line, no banned word over the app's own sentences (the analyst prose
/// blanked), the shape's keys (`portfolio-v42`, closing the v41 record's
/// offline-only limit on this branch).
#[test]
fn synthetic_role_risk_action_message_is_two_parts_with_no_app_concept() {
    let fx = synthetic_role_risk_fixture();
    let rr = synthetic_role_risk_verdict(&fx);
    let render = |verdict: &super::RoleRiskVerdict| {
        action_user_prompt(&ActionInput {
            dossier: &fx.dossier,
            subject: ActionSubject::RoleRisk { verdict },
            engine_set: &super::ROLE_RISK_ACTIONS,
            profile: &fx.dossier.profile,
        })
    };
    let user = render(&rr);
    let system = pipeline::action_system_prompt();
    let (part1, part2) = user
        .split_once("\n======== PART 2: TASK ========\n")
        .unwrap_or_else(|| panic!("no Part 2 marker\n{user}"));
    assert!(part1.starts_with("======== PART 1: INPUTS ========\nHOLDING\nBND ("), "{part1}");
    // The branch's own sections stay top-level, POSITION first after the
    // header, no COMPUTED heading (`portfolio-v71`, ruled 2026-10-08).
    let mut last = 0;
    for section in [
        "POSITION\n", "CLASS\n", "EXPOSURE TILT\n", "RISK PROFILE\n",
        "EVIDENCE GAPS\n", "VERDICT\n",
        "SUPPORTED ACTIONS\n", "INVESTOR PROFILE\n",
    ] {
        let key = format!("\n{section}");
        assert_eq!(part1.matches(&key).count(), 1, "Part 1 lacks {section}\n{part1}");
        let at = part1.find(&key).unwrap();
        assert!(at > last, "{section} out of order\n{part1}");
        last = at;
    }
    assert!(!part1.contains("\nCOMPUTED\n"), "{part1}");
    assert!(part1.contains("\nEVIDENCE GAPS\nno duration, credit or yield-curve data for this fund\n"), "{part1}");
    assert!(part1.contains("\nSUPPORTED ACTIONS\nThe rungs a fixed rule over the holding's reads supports, listed in full: sell-all, trim, hold. A rung not listed is outside that rule.\n"), "{part1}");
    assert!(
        part2.contains("Decide it from CLASS, VERDICT, EXPOSURE TILT and RISK PROFILE first, refined by EVIDENCE GAPS, POSITION, SUPPORTED ACTIONS and INVESTOR PROFILE."),
        "{part2}"
    );
    assert!(!part1.contains("Return "), "Part 1 instructs\n{part1}");
    assert!(!part2.contains("Name the returns you weighed"));
    for item in ["1. action — one rung for this holding", "2. rationale — one sentence", "RETURN SHAPE (every value is a placeholder)"] {
        assert!(part2.contains(item), "Part 2 lacks {item}\n{part2}");
    }
    assert!(!user.contains("CAPITAL EFFICIENCY") && !user.contains("PRICE TARGETS"), "{user}");
    // The role/risk VERDICT carries the document alone — no conviction, no
    // expected price line.
    assert!(part1.contains("\nVERDICT\nAn analyst's read of the holding's data and research: the thesis document.\nThesis document:\nRole: "), "{part1}");
    assert!(!part1.contains("Conviction:") && !part1.contains("Expected share price"), "{part1}");
    let mut blank = rr.clone();
    blank.thesis_document.clear();
    let blanked = render(&blank);
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

/// The research messages the harness renders: the gathering and synthesis
/// passes on TSLA's first stock topic and the synthetic fund's
/// exposure-profile topic, over the pipeline's own holding-constant brief
/// (`pipeline::research_brief`) and hand-written leads, write-ups and pages
/// (`research::samples`) — the prompts' shape, never a run's research.
fn research_samples() -> Vec<super::research::samples::Sample> {
    use super::research::samples;
    let f = fixtures().into_iter().find(|f| f.symbol == "TSLA").expect("TSLA");
    let d = dossier_of(&f, true);
    let mut brief = pipeline::research_brief(&d, rates(), None);
    brief.leads = samples::stock_leads(false);
    let mut continuity = pipeline::research_brief(&prompt_examples::continuity_dossier(&f), rates(), None);
    continuity.leads = samples::stock_leads(false);
    let agenda = super::research::build_agenda(&d, &super::research::AgendaTriggers::default());
    let fx = synthetic_role_risk_fixture();
    let fund_agenda =
        super::research::build_agenda(&fx.dossier, &super::research::AgendaTriggers::default());
    let exposure = fund_agenda
        .iter()
        .find(|t| t.key == "fund-exposure-profile")
        .expect("the retitled fund exposure topic (fix list 4.4)");
    let mut out = samples::gathering_messages("TSLA", &brief, &continuity, &agenda[0], false);
    out.extend(samples::synthesis_messages("TSLA", &brief, &agenda[0], false));
    out.push(samples::followup_ask_sample("TSLA", &brief, &agenda[0], false));
    let mut fund_brief = pipeline::research_brief(&fx.dossier, rates(), None);
    fund_brief.leads = samples::fund_leads(false);
    out.extend(
        samples::gathering_messages("BND", &fund_brief, &fund_brief, exposure, false)
            .into_iter()
            .take(1)
            .map(|mut s| {
                s.label = format!("{} (SYNTHETIC fund, exposure profile)", s.label);
                s
            }),
    );
    out
}

/// Every research message on the sample passes is two marked parts in order
/// with no app concept: Part 1 the input sections — the dated holding header,
/// FETCHED VALUES, on a gathering pass the leads and on a continuity run the
/// prior thesis document, the tier scale stated on a synthesis pass (on a
/// gathering pass it rides the tool descriptions) — and no instruction; Part
/// 2 the task — on a gathering pass the per-reply bound and the stopping
/// rule, on a synthesis pass the write-up with its length band and no return
/// shape — and neither part, nor the system prompt, carries a banned word or
/// a routing word; the follow-up ask is the one word `none` or the question.
/// The gathering brief less its PRIOR THESIS block: the prior document is the
/// model's own text, rendered verbatim by contract, so the lexicon checks read
/// the app's text around it.
fn without_prior_document(user: &str) -> String {
    let Some(start) = user.find("\nPRIOR THESIS") else {
        return user.to_string();
    };
    let rest = &user[start + 1..];
    let end = ["\nPAGES ALREADY RETRIEVED\n", "\nTOPIC\n"]
        .iter()
        .filter_map(|marker| rest.find(marker))
        .min()
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    format!("{}{}", &user[..start], &rest[end..])
}

#[test]
fn research_messages_are_two_parts_with_no_app_concept() {
    let samples = research_samples();
    assert_eq!(samples.len(), 10, "five gathering, three synthesis, the ask, one fund gathering");
    // The tool descriptions are prompt text too (`portfolio-v50`): they state
    // the tier scale and the page header's fields, and carry no banned word.
    let tools = super::research::research_tools().to_string();
    assert!(
        tools.contains("The source tier runs from 0 to 5: 0 is a primary source") && tools.contains("extraction quality"),
        "{tools}"
    );
    assert!(banned_hits(&tools).is_empty(), "tools carry {:?}\n{tools}", banned_hits(&tools));
    assert!(tools.contains("extraction quality (0 to 1"), "{tools}");
    assert!(tools.contains("the subjects its source is trusted on; and its extraction quality"), "{tools}");
    assert!(!tools.contains("tier holds"), "{tools}");
    assert!(!tools.contains("quoted material") && !tools.contains("defect of the source"), "{tools}");
    assert_no_routing_words("tools", &tools);
    for s in &samples {
        let (part1, part2) = s
            .user
            .split_once("======== PART 2: TASK ========")
            .unwrap_or_else(|| panic!("{}: no Part 2 marker\n{}", s.label, s.user));
        assert!(part1.starts_with("======== PART 1: INPUTS ========\nHOLDING\n"), "{}: {part1}", s.label);
        assert!(part1.contains("\nDate: 2026-09-16.\n"), "{}: no date line\n{part1}", s.label);
        assert!(part1.contains("\nFETCHED VALUES\n"), "{}: no FETCHED VALUES\n{part1}", s.label);
        assert!(part1.contains("\nTOPIC\n"), "{}: no TOPIC\n{part1}", s.label);
        assert!(s.format.is_none(), "{}: no research call carries a grammar", s.label);
        for gone in ["PRIOR FINDINGS", "CLAIMS SO FAR", "RETURN SHAPE", "followup_question", "fact_period"] {
            assert!(!s.user.contains(gone), "{}: {gone} survives\n{}", s.label, s.user);
        }
        if s.label.starts_with("gathering") {
            // `portfolio-v50`: Part 1 is inputs only — the tool results' fields are
            // glossed on the tool descriptions, not under a heading in Part 1.
            assert!(
                !part1.contains("TOOL RESULTS") && !part1.contains("0 is a primary source"),
                "{}: a tool-results legend in Part 1\n{part1}",
                s.label
            );
            assert!(s.tools.is_some(), "{}", s.label);
            // The holding-constant blocks lead: FETCHED VALUES before NEWS LEADS,
            // and the topic's own text after them.
            let at = |section: &str| part1.find(section).unwrap_or_else(|| panic!("{}: {section}", s.label));
            assert!(at("\nFETCHED VALUES\n") < at("\nNEWS LEADS\n") && at("\nNEWS LEADS\n") < at("\nTOPIC\n"), "{}: {part1}", s.label);
        } else {
            assert!(part1.contains("0 is a primary source"), "{}: the tier scale is unstated\n{part1}", s.label);
            assert!(part1.contains("source tier (0 to 5: 0 is a primary source"), "{}: the tier scale's range is unstated\n{part1}", s.label);
            let questions = if s.label.contains("disconfirming") { "the question" } else { "the questions" };
            assert!(
                part1.contains(&format!("\nEVIDENCE\nThe pages retrieved for {questions} under TOPIC, each under a header of: its id; its address; the published date, where the search reported one; when it was retrieved; its source tier (0 to 5:")),
                "{}: {part1}",
                s.label
            );
            assert!(part1.contains("and its extraction quality (0 to 1: the article text recovered against a full article's worth). A page marked stub did not yield its article"), "{}: {part1}", s.label);
            assert!(!part1.contains("defect of the source") && !part1.contains("pages shown"), "{}: {part1}", s.label);
            assert!(part2.contains("it does not exclude it, and a figure that cannot be right is a defect of the source."), "{}: {part2}", s.label);
            assert!(!part1.contains("tier holds"), "{}: {part1}", s.label);
            assert!(
                part2.contains("Weigh each page by its source tier and extraction quality") && !part2.contains("applies to the subjects"),
                "{}: {part2}",
                s.label
            );
            assert!(part1.contains("=== S1: "), "{}: {part1}", s.label);
            assert!(!part1.contains("\nNEWS LEADS\n"), "{}: a synthesis carries no leads", s.label);
            // The write-up task: plain text, the length band, and the source
            // clause naming FETCHED VALUES since the brief carries it.
            assert!(part2.contains("as plain text — no code fence, no JSON, no heading before the first line"), "{}: {part2}", s.label);
            assert!(part2.contains("The write-up runs 400 to 900 words."), "{}: {part2}", s.label);
            assert!(part2.contains("or FETCHED VALUES where the figure comes from there"), "{}: {part2}", s.label);
            assert!(s.tools.is_none(), "{}", s.label);
            assert!(!s.system.to_lowercase().contains("json") && s.system.contains("Part 2 says what the write-up covers"), "{}: {}", s.label, s.system);
        }
        assert!(!part1.contains("recency"), "{}: recency rendered\n{part1}", s.label);
        let app_text = without_prior_document(&s.user);
        let app_part1 = app_text.split("======== PART 2: TASK ========").next().unwrap_or("");
        assert!(
            !app_part1.contains("Return ") && !app_part1.to_lowercase().contains("your "),
            "{}: Part 1 instructs\n{app_part1}",
            s.label
        );
        for (label, text) in [("system", s.system.as_str()), ("user", app_text.as_str())] {
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
                !app_text.contains(word) && !s.system.contains(word),
                "{}: {word} leaked\n{}",
                s.label,
                app_text
            );
        }
        if s.label.starts_with("gathering") {
            assert!(!s.user.contains("Replies remaining"), "{}: {}", s.label, s.user);
            assert_eq!(s.appended.len(), 1);
            assert_eq!(s.appended[0].role, "user");
            assert_eq!(
                s.appended[0].content,
                "SEARCHING\nReplies remaining, including this one: 8.\nPages fetched on the last reply are kept.\n"
            );
            assert!(banned_hits(&s.appended[0].content).is_empty());
            assert_no_routing_words(&s.label, &s.appended[0].content);
            if s.label.contains("previously retrieved pages") {
                assert!(part1.contains("PAGES ALREADY RETRIEVED") && part1.contains("BEGIN PAGE TEXT"));
                assert!(
                    part1.contains("\nPAGES ALREADY RETRIEVED\nPages retrieved while researching this holding, each as web_fetch returns it.\n"),
                    "{}: {part1}",
                    s.label
                );
                assert!(part1.contains("retrieved 2026-09-16T") && !part1.contains("retrieved 2026-09-17"), "{}: {part1}", s.label);
                assert!(
                    part2.contains("1. Read the pages under PAGES ALREADY RETRIEVED against the questions. Search for what remains unanswered"),
                    "{}: {part2}",
                    s.label
                );
            } else if s.label.contains("follow-up pass") {
                assert!(
                    part2.contains("1. Search for what the question under FOLLOW-UP asks") && !part2.contains("Read the pages"),
                    "{}: {part2}",
                    s.label
                );
                assert!(part2.contains("where the question allows;") && !part2.contains("the questions"), "{}: {part2}", s.label);
                assert!(
                    part2.contains("under HOLDING. The questions under TOPIC are what that question serves; this pass does not search them.\n"),
                    "{}: {part2}",
                    s.label
                );
                assert!(part1.contains("\nFOLLOW-UP\nThe question this pass pursues.\n"), "{}: {part1}", s.label);
                assert!(part1.contains("\nWRITE-UP SO FAR\nThe topic's write-up from its earlier passes.\n"), "{}: {part1}", s.label);
            } else if !s.label.contains("disconfirming") {
                assert!(
                    part2.contains("1. Search for what the questions ask") && !part2.contains("Read the pages"),
                    "{}: {part2}",
                    s.label
                );
            } else {
                assert!(
                    part2.contains("Find what the web shows on the question under TOPIC for this holding, as of the date under HOLDING. The write-ups under WRITE-UPS SO FAR are what that question tests: search for evidence against them, not for more evidence for them."),
                    "{}: {part2}",
                    s.label
                );
                assert!(
                    part2.contains("1. Search for what the question asks, then fetch and read the results and the leads under NEWS LEADS most likely to answer it."),
                    "{}: {part2}",
                    s.label
                );
                assert!(part2.contains("where the question allows;") && !part2.contains("the questions"), "{}: {part2}", s.label);
                assert!(
                    part1.contains("\nWRITE-UPS SO FAR\nThis run's write-ups on the holding, each under its topic.\n"),
                    "{}: {part1}",
                    s.label
                );
            }
            assert!(!part2.contains("worth fetching"), "{}: {part2}", s.label);
            assert!(part2.contains("2. At most 8 tool calls in one reply."), "{}: {part2}", s.label);
            if s.label.contains("follow-up pass") {
                assert!(part2.contains("3. Stop when the question under FOLLOW-UP is answered, or when what remains cannot be found:"), "{}: {part2}", s.label);
            } else if s.label.contains("disconfirming") {
                assert!(part2.contains("3. Stop when the question is answered, or when what remains cannot be found:"), "{}: {part2}", s.label);
            } else {
                assert!(part2.contains("3. Stop when the questions are answered"), "{}: {part2}", s.label);
            }
            assert!(part2.contains("a weak source lowers confidence"), "{}: {part2}", s.label);
            assert!(
                part2.contains("Prefer a source tier nearer 0 and an extraction quality nearer 1 where") && !part2.contains("lower tier number"),
                "{}: {part2}",
                s.label
            );
            assert!(!part2.contains("applies to the subjects"), "{}: {part2}", s.label);
            assert!(part2.contains("a figure that cannot be right is a defect of the source"), "{}: {part2}", s.label);
            assert!(!part2.contains("PRIOR"), "{}: Part 2 points at a prior block\n{part2}", s.label);
        }
        if s.label.contains("continuity") {
            assert!(part1.contains("\nPRIOR THESIS (written 2026-09-02)\n") && !part1.contains("STANDING CONDITIONS"), "{}: {part1}", s.label);
            let at = |section: &str| part1.find(section).unwrap_or_else(|| panic!("{}: {section}", s.label));
            assert!(at("\nNEWS LEADS\n") < at("\nPRIOR THESIS") && at("\nPRIOR THESIS") < at("\nTOPIC\n"), "{}: {part1}", s.label);
        } else {
            assert!(!part1.contains("PRIOR THESIS"), "{}: a debut carries no prior\n{part1}", s.label);
        }
        if s.label.starts_with("synthesis") {
            assert!(!part1.contains("SEARCHING") && !part2.contains("SEARCHING"), "{}", s.label);
        }
        if s.label.starts_with("synthesis") && s.label.contains("follow-up pass") {
            assert!(
                part2.contains("Rewrite the write-up under WRITE-UP SO FAR whole as plain text — no code fence, no JSON, no heading before the first line, folding in what EVIDENCE shows on the question under FOLLOW-UP, so the topic has one write-up:"),
                "{}: {part2}",
                s.label
            );
            assert!(part1.contains("\nFOLLOW-UP\n") && part1.contains("\nWRITE-UP SO FAR\n"), "{}: {part1}", s.label);
        }
        if s.label.starts_with("synthesis") && s.label.contains("disconfirming") {
            assert!(
                part2.contains("It states how EVIDENCE bears on the write-ups under WRITE-UPS SO FAR: which it contradicts or weakens and how, which it leaves standing, and any contrary evidence that stands on its own"),
                "{}: {part2}",
                s.label
            );
            assert!(part1.contains("\nWRITE-UPS SO FAR\nThis run's write-ups on the holding, each under its topic.\n"), "{}: {part1}", s.label);
        }
        if s.label.contains("the follow-up ask") {
            // The conversation's second message: the write-up echoed as the
            // assistant's turn, then the ask; the reply is read as `none` or
            // the question, and the ask names no app concept.
            assert_eq!(s.appended.len(), 2, "{}", s.label);
            assert_eq!(s.appended[0].role, "assistant");
            assert_eq!(s.appended[1].role, "user");
            assert_eq!(s.appended[1].content, super::research::followup_ask());
            assert!(s.appended[1].content.contains("or the one word none"), "{}", s.appended[1].content);
            assert!(banned_hits(&s.appended[1].content).is_empty());
            assert_no_routing_words(&s.label, &s.appended[1].content);
            assert!(s.stage.ends_with(" synthesis follow-up"), "{}", s.stage);
        } else if s.label.starts_with("synthesis") {
            assert!(s.appended.is_empty(), "{}", s.label);
        }
        assert!(
            !s.user.contains("FOLLOW-UP question") && !s.user.contains("TOPIC questions"),
            "{}: a heading used as an adjective\n{}",
            s.label,
            s.user
        );
    }
    // The tool results carry no instruction either.
    for (label, text) in super::research::samples::tool_results(false) {
        assert!(
            !text.contains("Work with what you have") && !text.contains("contributes no evidence"),
            "{label}: {text}"
        );
        assert!(banned_hits(&text).is_empty(), "{label}: {text}");
    }
}

/// Every rendered fixed-set prompt to one Markdown file for a human read
/// (`MARKET_SIGNAL_LOCAL_EVAL_PROMPT_DUMP=<file>`): the system prompts once,
/// then per holding the thesis-document message, the appendix ask, the action
/// message, and the lines the tax and cost variants change, then the synthetic
/// role/risk case, then the research messages.
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
    out.push_str("## 1. System prompts\n\n### 1a. Thesis document — stock\n\n");
    out.push_str(&fence(&pipeline::thesis_system_prompt(false)));
    out.push_str("### 1b. Thesis document — fund\n\n");
    out.push_str(&fence(&pipeline::thesis_system_prompt(true)));
    out.push_str("### 1c. The appendix ask (the conversation's second message)\n\n");
    out.push_str(&fence(&pipeline::appendix_user_prompt()));
    out.push_str("### 1d. Action\n\n");
    out.push_str(&fence(&pipeline::action_system_prompt()));
    let mut n = 2;
    for f in fixtures() {
        let VerdictDisposition::Priced(graded) = &f.disposition else { continue };
        let d = dossier_of(&f, true);
        let interp = thesis_user_prompt(&thesis_input(&f, &d));
        let engine_set = engine::feasible_actions(f.engine_output.grade, &f.engine_output.hurdle, None, false);
        macro_rules! action_input { ($dd:expr) => { ActionInput {
            dossier: $dd,
            subject: ActionSubject::Priced { graded, engine: &f.engine_output, pre_profit: None },
            engine_set: &engine_set, profile: &$dd.profile,
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
        out.push_str(&format!("### {n}a. Thesis-document message ({} chars)\n\n", interp.len()));
        out.push_str(&fence(&interp));
        out.push_str(&format!("### {n}b. Action message ({} chars)\n\n", list.len()));
        out.push_str(&fence(&list));
        out.push_str(&format!("### {n}c. Tax-exempt variant: lines that differ from the action message\n\n"));
        out.push_str(&fence(&diff_lines(&list, &tax)));
        out.push_str(&format!("### {n}d. Cost-basis ×3 variant: lines that differ from the action message\n\n"));
        out.push_str(&fence(&diff_lines(&list, &cost)));
        n += 1;
    }
    // The synthetic role/risk case (`portfolio-v42`): the system prompt, the
    // debut message with the hand-written research, the continuity message the
    // pipeline renders on a stub second run, and the action message on the
    // stub's verdict.
    let fx = synthetic_role_risk_fixture();
    let debut = role_risk_user_prompt(&synthetic_role_risk_input(&fx));
    let (cont_system, cont) = synthetic_role_risk_continuity_messages();
    let rr = synthetic_role_risk_verdict(&fx);
    let action = action_user_prompt(&ActionInput {
        dossier: &fx.dossier,
        subject: ActionSubject::RoleRisk { verdict: &rr },
        engine_set: &super::ROLE_RISK_ACTIONS,
        profile: &fx.dossier.profile,
    });
    out.push_str(&format!(
        "## {n}. BND (SYNTHETIC role/risk bond fund — hand-written closes, positioning and research; the fixed set's house view; engine set [sell-all, trim, hold]; no hurdle)\n\n"
    ));
    out.push_str(&format!("### {n}a. Role/risk system prompt\n\n"));
    out.push_str(&fence(&cont_system));
    out.push_str(&format!("### {n}b. Role/risk thesis-document message — first analysis ({} chars)\n\n", debut.len()));
    out.push_str(&fence(&debut));
    out.push_str(&format!("### {n}c. Role/risk thesis-document message — continuity on the stub's first run ({} chars; the analysis is the offline stub's)\n\n", cont.len()));
    out.push_str(&fence(&cont));
    out.push_str(&format!("### {n}d. Action message on the stub's verdict ({} chars)\n\n", action.len()));
    out.push_str(&fence(&action));
    // The research messages: the gathering and synthesis passes rendered on
    // TSLA's first stock topic and the synthetic fund's exposure-profile topic
    // over hand-written leads, write-ups and pages, then what a gathering
    // turn gets back.
    let n = n + 1;
    out.push_str(&format!(
        "## {n}. Research messages (TSLA's competitive-position topic and the SYNTHETIC fund's exposure-profile topic; hand-written leads, write-ups and pages)\n\n"
    ));
    let mut letter = b'a';
    for s in research_samples() {
        out.push_str(&format!("### {n}{}. {} — system prompt\n\n", letter as char, s.label));
        out.push_str(&fence(&s.system));
        letter += 1;
        out.push_str(&format!("### {n}{}. {} — message ({} chars)\n\n", letter as char, s.label, s.user.len()));
        out.push_str(&fence(&s.user));
        for message in &s.appended {
            out.push_str(&format!("\nAppended {} message:\n\n", message.role));
            out.push_str(&fence(&message.content));
        }
        letter += 1;
    }
    for (label, text) in super::research::samples::tool_results(false) {
        out.push_str(&format!("### {n}{}. Tool result — {label}\n\n", letter as char));
        out.push_str(&fence(&text));
        letter += 1;
    }
    std::fs::write(&path, out).expect("write prompt dump");
    println!("wrote {path}");
}
