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
    validate_ledger_rewrite_with_research, ActionInput, ActionSubject, InterpretationInput,
    RoleRiskInput,
};
use super::{
    Action, AssetClass, ContinuityStamps, FalsifierDraft, LedgerDraft, QuantCoreDraft,
    LedgerBranch, StatementBasis, TriggerDraft, VerdictDisposition,
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
    spot: f64,
    /// The dated close the run authored against — seeded as the one daily
    /// close of the reconstructed financials, so the market-data series
    /// resolve an observation in the live harness.
    authoring_close: engine::DatedValue,
    synthetic_position: SyntheticPosition,
    conditions: Vec<FixtureCondition>,
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

fn stamps_of(f: &Fixture) -> ContinuityStamps {
    ContinuityStamps {
        statement_basis: f.statement_basis,
        equity_source: None,
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
    d
}

fn expected_class(expect: &str) -> Option<&str> {
    expect.strip_prefix("downgraded:")
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
        let activity = format!(
            "put/call vol {:.3}, put/call OI {:.3}, IV {:.3}, IV skew 0.000",
            options.put_call_volume.expect("persisted volume"),
            options.put_call_open_interest.expect("persisted OI"),
            options.implied_volatility.expect("persisted IV"),
        );
        assert_eq!(options.iv_skew, Some(0.0), "{}: fixture skew", f.symbol);
        assert!(prompt.contains(&activity), "{}: {activity}", f.symbol);
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
        let render = |d: &super::dossier::HoldingDossier| {
            action_user_prompt(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded, engine: &f.engine_output, pre_profit: None },
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
        // these, and the fixture's model-authored summaries are scrubbed of them.
        let lower = taxable.to_lowercase();
        for absent in ["cost basis", "unrealized", "quantity:", "market value", "p/l", "- tax", "share-equivalents"] {
            assert!(!lower.contains(absent), "{}: {absent} leaked: {taxable}", f.symbol);
        }
        assert_eq!(taxable.matches("ENGINE SET").count(), 1, "{}", f.symbol);
        assert!(taxable.contains("higher is better on every axis"), "{}", f.symbol);
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
        assert!(taxable.starts_with(&format!("HOLDING: {} (", f.symbol)), "{taxable}");
        assert!(taxable.contains("Position change since last run: NEW (no prior verdict"), "{taxable}");
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

    println!("\n== fixed evidence, live (RECONSTRUCTED packets, not a replay of attempt 6) — {repeats} repeat(s) ==");
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
        let mut fresh: Option<super::Interpretation> = None;
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
                    ("target rationale", &interp.model_target_rationale),
                    ("financial summary", &interp.financial_summary),
                    ("self-assessment", &interp.self_assessment),
                    ("what changed", &interp.what_changed),
                ] {
                    println!("    {label}: {}", text.replace('\n', " "));
                    let lower = text.to_lowercase();
                    if let Some(phrase) = ACCOUNT_ECONOMICS_PHRASES.iter().find(|p| lower.contains(**p)) {
                        println!("    !! account-economics phrase in {label}: \"{phrase}\"");
                    }
                }
                println!("    what_changed_entries: {} (debut: app-written)", interp.what_changed_entries.len());
            }
            if fresh.is_none() {
                fresh = Some(interp.clone());
            }
            let (ledger, audit) = validate_ledger_rewrite_with_research(
                &interp.ledger, None, None, LedgerBranch::Priced, f.is_fund, None, Some(f.spot),
                &HashSet::new(), true, stamps_of(&f),
            );
            let kept = ledger.conditions.iter().filter(|c| c.quant.is_some()).count();
            let qualitative = ledger.conditions.iter().filter(|c| c.quant.is_none() && c.downgraded_reason.is_none()).count();
            println!(
                "  interpretation #{r}: {:.0}s — {} conditions: {kept} quantitative kept, {qualitative} authored qualitative, {} downgraded",
                started.elapsed().as_secs_f64(), ledger.conditions.len(), audit.downgraded.len()
            );
            for c in &ledger.conditions {
                match (&c.quant, &c.downgraded_reason) {
                    (Some(q), _) => println!(
                        "    KEPT   [{:?}{}] {} {} {} (margin {}) <= \"{}\"",
                        c.role, c.trigger_family.map(|f| format!(" {f:?}")).unwrap_or_default(),
                        q.series.as_kebab(), q.comparator.as_kebab(), q.threshold, q.margin, c.statement
                    ),
                    (None, Some(reason)) => println!("    DOWN   \"{}\" — {reason}", c.statement),
                    (None, None) => println!("    QUAL   \"{}\"", c.statement),
                }
            }
        }

        // Action, N repeats, then the tax and cost variants on the fixed verdict,
        // then one call on the verdict assembled from the fresh interpretation
        // (the pipeline's own assembly), so clean interpretation prose is seen
        // reaching the rung.
        let engine_set = engine::feasible_actions(f.engine_output.grade, &f.engine_output.hurdle, None, false);
        let decide = |d: &super::dossier::HoldingDossier, subject: &super::GradedVerdict, label: &str| {
            let started = std::time::Instant::now();
            match analyst.decide_action(&ActionInput {
                dossier: d,
                subject: ActionSubject::Priced { graded: subject, engine: &f.engine_output, pre_profit: None },
                engine_set: &engine_set,
                changes: None,
                profile: &d.profile,
            }) {
                Ok(decision) => println!(
                    "  action {label}: {} ({:.0}s; set [{}]; run chose {}) — {}",
                    decision.action.as_kebab(), started.elapsed().as_secs_f64(),
                    engine_set.iter().map(Action::as_kebab).collect::<Vec<_>>().join(", "),
                    graded.action.as_kebab(), decision.rationale
                ),
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
        if let Some(interp) = fresh {
            let assembled = pipeline::graded_verdict_from_interpretation(
                &f.engine_output,
                d.options_signal.clone(),
                interp,
                graded.engine_view.clone(),
            );
            decide(&d, &assembled, "fresh-interpretation verdict");
        }
        ctx.step_finished(step_key, "ok", None);
    }
}
