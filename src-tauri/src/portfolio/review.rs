//! The self-review (`docs/portfolio-workflow.md` §Step 6e;
//! `docs/portfolio-analysis.md` §The per-holding pipeline, §Outcome learning):
//! on a continuity run the reasoner reviews the prior position against what has
//! happened since, before this run's thesis document is written. This module
//! holds what the review reads — PRIOR POSITION and the REALIZED block, both
//! computed deterministically off the dossier, the engine's output and the
//! holding's accuracy record — and the message itself. The review is prose,
//! read as text and validated by nothing; this run's thesis-document message is
//! its only reader.

use chrono::NaiveDate;

use crate::portfolio::distill::DistillPrompt;
use crate::portfolio::dossier::HoldingDossier;
use crate::portfolio::engine::{self, DatedValue, EngineOutput, RateAnchors, ThenNow};
use crate::portfolio::outcome::{self, CheckOutcome, Horizon, HorizonPrices, PriceRecord, UnscorableCause};
use crate::portfolio::pipeline::{self, SplitContext};
use crate::portfolio::{ActionSource, AnalysisRecord, GradedVerdict, VerdictDisposition};

/// The review's drafted length band, stated in the prompt and never checked
/// by the app (`docs/portfolio-analysis.md` §Starting parameters).
pub const REVIEW_WORDS: (u32, u32) = (400, 900);

/// The branch this run's engine stage took — what the REALIZED block compares.
pub enum ReviewSubject<'a> {
    /// A priced stock or fund: the engine's output this run.
    Priced { engine: &'a EngineOutput },
    /// A `role_risk_only` fund: its realized block reads the fund's own
    /// basis then and now off the dossier.
    RoleRisk,
}

/// What the self-review reads (`docs/portfolio-workflow.md` §Step 6e): the
/// dossier (the prior position, the prior thesis document and the accuracy
/// record ride it), the run's Treasury prints FETCHED VALUES states, this
/// run's analysis, the prior document's split-context line, the split bridge
/// since the prior pass and this run's engine subject.
pub struct ReviewInput<'a> {
    pub dossier: &'a HoldingDossier,
    pub rates: &'a RateAnchors,
    pub analysis: &'a AnalysisRecord,
    pub prior_split: Option<SplitContext>,
    /// The split-adjustment bridge factor since the prior pass — `Some(1.0)`
    /// where the series was not re-based (or the prior carries no anchor),
    /// `None` where the prior anchor's bar is missing from the fresh window,
    /// so every prior price is withheld rather than compared cross-basis.
    pub price_bridge: Option<f64>,
    pub subject: ReviewSubject<'a>,
}

impl ReviewInput<'_> {
    /// The stage label the review call issues under.
    pub fn stage(&self) -> String {
        format!("review {}", self.dossier.position.symbol)
    }
}

/// The rendered review prompt: the role-line system message and the two-part
/// user message.
pub type ReviewPrompt = DistillPrompt;

const PART_1: &str = "======== PART 1: INPUTS ========\n";
const PART_2: &str = "\n======== PART 2: TASK ========\n\n";

/// The review message (`docs/portfolio-workflow.md` §Step 6e): one message in
/// two parts. Part 1, in page order: the holding header with the analysis
/// date, FETCHED VALUES, PRIOR POSITION, PRIOR THESIS verbatim with any
/// split-context line, ANALYSIS (this run's — the prior analysis stays out,
/// already folded into it) and REALIZED. Part 2 asks for the review, within
/// its length band.
pub fn review_prompt(input: &ReviewInput) -> ReviewPrompt {
    let system = "You are an investment analyst reviewing the prior position on one holding for \
a portfolio review. Part 1 of the message gives the inputs. Part 2 says what the review covers \
and how to return it."
        .to_string();
    let d = input.dossier;
    let mut user = String::from(PART_1);
    user.push_str(&pipeline::holding_header(d));
    user.push_str(&pipeline::fetched_values_section(d, input.rates));
    let position = prior_position_section(input);
    user.push_str(&position.text);
    user.push_str(&pipeline::prior_thesis_section(d, input.prior_split));
    user.push_str(&pipeline::analysis_section(d, input.analysis));
    user.push_str(&realized_section(input));
    user.push_str(&task_section(input, &position));
    DistillPrompt { system, user }
}

/// The offline stub's review — a deterministic stand-in for the model's
/// prose, so a stub pipeline still hands the thesis-document message a
/// REVIEW on a continuity run.
pub fn stub_review(input: &ReviewInput) -> String {
    let symbol = &input.dossier.position.symbol;
    match input.subject {
        ReviewSubject::Priced { .. } => format!(
            "[stub: the review of the prior position on {symbol} — each expected price and each \
             falsifier and trigger against what happened, as the review call returned it]"
        ),
        ReviewSubject::RoleRisk => format!(
            "[stub: the review of the prior action on {symbol} — each trigger against what \
             happened, as the review call returned it]"
        ),
    }
}

// ---- PRIOR POSITION ----------------------------------------------------------

/// The rendered PRIOR POSITION and what it put on record, which Part 2's first
/// item points at.
struct PriorPosition {
    text: String,
    /// Some expected price is on record under PRIOR POSITION.
    has_prices: bool,
    /// The prior is a role read — an action and its rationale alone.
    role_read: bool,
}

/// The prior run's ET session date — the date its position was stated.
fn prior_session(d: &HoldingDossier) -> Option<NaiveDate> {
    d.prior_vintage.as_deref().and_then(crate::market_clock::et_date_of)
}

fn run_session(d: &HoldingDossier) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&d.analysis_date, "%Y-%m-%d").ok()
}

fn iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn horizon_words(h: Horizon) -> &'static str {
    match h {
        Horizon::ThreeMonth => "three months",
        Horizon::TwelveMonth => "twelve months",
        Horizon::ThreeYear => "three years",
    }
}

/// A prior price-denominated value on today's basis: the stored value times
/// the bridge factor, marked where the factor moved it; `None` where the
/// bridge was unresolvable (withheld, never compared cross-basis).
fn bridged(value: f64, bridge: Option<f64>) -> Option<(f64, bool)> {
    bridge.map(|f| (value * f, f != 1.0))
}

fn money_marked(v: Option<(f64, bool)>) -> String {
    match v {
        Some((p, true)) => format!("${p:.2} (bridged)"),
        Some((p, false)) => format!("${p:.2}"),
        None => "withheld".to_string(),
    }
}

/// A per-share price as PRIOR POSITION states it — "withheld" alone where the
/// bridge withheld it.
fn per_share(v: Option<(f64, bool)>) -> String {
    match v {
        Some(_) => format!("{} per share", money_marked(v)),
        None => money_marked(v),
    }
}

/// The one-sentence gloss for the bridge's marks, where they apply: a value
/// withheld where the bridge was unresolvable, a value marked bridged where a
/// split re-based the series; none where the series was not re-based.
fn basis_gloss(bridge: Option<f64>) -> &'static str {
    match bridge {
        None => {
            " A price shown as withheld could not be put on today's price basis: whether a share \
             split re-based the price series since the prior analysis could not be verified this \
             run."
        }
        Some(f) if f != 1.0 => {
            " A price marked bridged was converted onto today's price basis across a share split \
             since the prior analysis."
        }
        Some(_) => "",
    }
}

/// The action line PRIOR POSITION carries — chosen in the prior analysis with
/// its rationale less the app's caveat, or set by rule after it with none
/// (`docs/portfolio-analysis.md` §Portfolio action, mirrored).
fn action_lines(prior: &crate::portfolio::HoldingVerdict) -> String {
    let Some(action) = crate::portfolio::carried_action(prior) else {
        return String::new();
    };
    match prior.action_source {
        ActionSource::ModelChosen => {
            let mut p = format!("Action: {}, chosen in the prior analysis.\n", action.as_kebab());
            let rationale = crate::portfolio::carried_rationale(prior)
                .map(pipeline::investment_sentence)
                .unwrap_or("")
                .trim();
            if !rationale.is_empty() {
                p.push_str(&format!("Rationale: {rationale}\n"));
            }
            p
        }
        ActionSource::RuleDemoted => format!(
            "Action: {}, set by rule after the prior analysis, not chosen in it.\n",
            action.as_kebab()
        ),
    }
}

/// PRIOR POSITION (`docs/portfolio-workflow.md` §Step 6e): on a priced prior,
/// the prior run's date and the price then, its action and rationale, its
/// conviction and its three expected prices each with its horizon date, every
/// price on today's basis; on a role read, the action and rationale alone;
/// after an abstention, the retained document's date and price then, and that
/// a later analysis made no call, so nothing else is on record.
fn prior_position_section(input: &ReviewInput) -> PriorPosition {
    let d = input.dossier;
    let Some(prior) = d.prior_verdict.as_ref() else {
        return PriorPosition { text: String::new(), has_prices: false, role_read: false };
    };
    let date = prior_session(d).map(iso).unwrap_or_else(|| "(gap)".into());
    let basis_gloss = basis_gloss(input.price_bridge);
    match &prior.disposition {
        VerdictDisposition::Priced(g) => {
            let mut p = format!(
                "\nPRIOR POSITION\nThe position stated at the prior analysis: its date, the price \
                 then, the action with its rationale, the conviction, and the expected share \
                 price at each horizon with the horizon's date.{basis_gloss}\n"
            );
            p.push_str(&format!("Date: {date}.\n"));
            p.push_str(&format!(
                "Price then: {}.\n",
                d.prior_spot
                    .map(|s| per_share(bridged(s, input.price_bridge)))
                    .unwrap_or_else(|| "(gap)".into())
            ));
            p.push_str(&action_lines(prior));
            p.push_str(&format!(
                "Conviction: {}.\n",
                g.appendix.conviction.map(|c| c.as_str()).unwrap_or("none stated")
            ));
            let created = prior_session(d);
            for (h, price) in prior_prices(g) {
                let when = created
                    .and_then(|c| outcome::horizon_date(c, h))
                    .map(iso)
                    .unwrap_or_else(|| "(gap)".into());
                match price.map(|v| bridged(v, input.price_bridge)) {
                    Some(Some(v)) => p.push_str(&format!(
                        "Expected price at {}: {} by {when}.\n",
                        horizon_words(h),
                        money_marked(Some(v))
                    )),
                    Some(None) => p.push_str(&format!(
                        "Expected price at {}: withheld ({when}).\n",
                        horizon_words(h)
                    )),
                    None => p.push_str(&format!(
                        "Expected price at {}: none stated ({when}).\n",
                        horizon_words(h)
                    )),
                }
            }
            PriorPosition {
                text: p,
                has_prices: g.appendix.expected_prices().iter().any(|(_, v)| v.is_some()),
                role_read: false,
            }
        }
        VerdictDisposition::RoleRiskOnly(_) => {
            let mut p = String::from(
                "\nPRIOR POSITION\nThe action taken at the prior analysis, with its rationale.\n",
            );
            p.push_str(&action_lines(prior));
            PriorPosition { text: p, has_prices: false, role_read: true }
        }
        VerdictDisposition::InsufficientEvidence { .. } => {
            let mut p = format!(
                "\nPRIOR POSITION\nThe position behind the document under PRIOR THESIS: its date \
                 and the price then.{basis_gloss}\n"
            );
            p.push_str(&format!("Date: {date}.\n"));
            match &d.prior_authoring_close {
                Some(anchor) => match bridged(anchor.value, input.price_bridge) {
                    Some(v) => p.push_str(&format!(
                        "Price then: {} (the close on {}).\n",
                        per_share(Some(v)),
                        anchor.date
                    )),
                    None => p.push_str("Price then: withheld.\n"),
                },
                None => p.push_str("Price then: (gap).\n"),
            }
            // The abstention's own reason is the app's card text, never the
            // model's: the line states the fact alone (ruled 2026-10-09).
            p.push_str(
                "A later analysis made no call, so no action, conviction or expected price is on \
                 record; the document under PRIOR THESIS states its own.\n",
            );
            PriorPosition { text: p, has_prices: false, role_read: false }
        }
        VerdictDisposition::NotRated { .. } => {
            PriorPosition { text: String::new(), has_prices: false, role_read: false }
        }
    }
}

/// A priced prior's three expected prices in horizon order.
fn prior_prices(g: &GradedVerdict) -> [(Horizon, Option<f64>); 3] {
    [
        (Horizon::ThreeMonth, g.appendix.expected_price_3m),
        (Horizon::TwelveMonth, g.appendix.expected_price_12m),
        (Horizon::ThreeYear, g.appendix.expected_price_3y),
    ]
}

// ---- REALIZED ----------------------------------------------------------------

/// The price now — the holding header's quote — where usable.
fn price_now(d: &HoldingDossier) -> Option<f64> {
    d.financials.current_price.filter(|p| p.is_finite() && *p > 0.0)
}

/// The close the move since the prior run is measured from: the newest bar
/// strictly before the prior run's session in this run's own series — the
/// prior vintage's anchor session on today's basis.
fn move_base(d: &HoldingDossier) -> Option<&DatedValue> {
    let since = iso(prior_session(d)?);
    d.financials
        .daily_closes
        .iter()
        .rev()
        .find(|c| c.date.as_str() < since.as_str() && c.value.is_finite() && c.value > 0.0)
}

/// The move since the prior run: spot now over the base close, less one.
fn move_since_prior(d: &HoldingDossier) -> Option<f64> {
    Some(price_now(d)? / move_base(d)?.value - 1.0)
}

/// The highest and the lowest close since the prior run's session, each with
/// its date — the path so far.
fn path_since(d: &HoldingDossier) -> Option<(&DatedValue, &DatedValue)> {
    let since = iso(prior_session(d)?);
    let closes: Vec<&DatedValue> = d
        .financials
        .daily_closes
        .iter()
        .filter(|c| c.date.as_str() >= since.as_str() && c.value.is_finite() && c.value > 0.0)
        .collect();
    let high = closes.iter().copied().max_by(|a, b| a.value.total_cmp(&b.value))?;
    let low = closes.iter().copied().min_by(|a, b| a.value.total_cmp(&b.value))?;
    Some((high, low))
}

fn cause_words(cause: UnscorableCause) -> String {
    match cause {
        UnscorableCause::NoForecast => "no price was stated at this horizon".to_string(),
        UnscorableCause::NoAnchor => {
            "the forecast carries no close to put its prices on today's basis".to_string()
        }
        UnscorableCause::AnchorBarMissing => "the close its prices were anchored to is missing \
             from the fetched series, so its basis could not be verified"
            .to_string(),
        UnscorableCause::NoCloseInProximity => format!(
            "no close within {} sessions before the date",
            outcome::HORIZON_PROXIMITY_SESSIONS
        ),
    }
}

/// The capital-efficiency state as the prompt names it.
fn hurdle_words(h: crate::portfolio::HurdleState) -> &'static str {
    match h {
        crate::portfolio::HurdleState::Clears => "clears",
        crate::portfolio::HurdleState::Indeterminate => "indeterminate",
        crate::portfolio::HurdleState::Fails => "fails",
        crate::portfolio::HurdleState::Unscorable => "unscorable",
    }
}

/// The REALIZED block (`docs/portfolio-workflow.md` §Step 6e) for the branch
/// this run's engine stage took.
fn realized_section(input: &ReviewInput) -> String {
    match &input.subject {
        ReviewSubject::Priced { engine } => priced_realized(input, engine),
        ReviewSubject::RoleRisk => role_risk_realized(input),
    }
}

fn priced_realized(input: &ReviewInput, engine_now: &EngineOutput) -> String {
    let d = input.dossier;
    let mut p = String::from(
        "\nREALIZED\nWhat has happened since the prior analysis, computed from the fetched prices \
         and the stored records, under the labels below.\n",
    );
    p.push_str(&price_block(d));
    if let Some(VerdictDisposition::Priced(g)) = d.prior_verdict.as_ref().map(|v| &v.disposition) {
        p.push_str(&expected_prices_block(input, g));
    }
    p.push_str(&accuracy_block(d));
    if let Some(VerdictDisposition::Priced(g)) = d.prior_verdict.as_ref().map(|v| &v.disposition) {
        p.push_str(&computed_then_now_block(input, g, engine_now));
    }
    p
}

/// PRICE: the price now, its move since the prior run against the base close,
/// and the path since — the highest and lowest closes with their dates.
fn price_block(d: &HoldingDossier) -> String {
    let mut p = String::from("\nPRICE\n");
    match (price_now(d), move_base(d)) {
        (Some(now), Some(base)) => p.push_str(&format!(
            "Now: ${now:.2} per share, {:+.1}% since the prior analysis (from the close of \
             ${:.2} on {}).\n",
            (now / base.value - 1.0) * 100.0,
            base.value,
            base.date
        )),
        (Some(now), None) => p.push_str(&format!(
            "Now: ${now:.2} per share; no close before the prior analysis is in the fetched \
             series, so the move since is not computed.\n"
        )),
        (None, _) => p.push_str("Now: (gap).\n"),
    }
    if let Some((high, low)) = path_since(d) {
        p.push_str(&format!(
            "Closes since the prior analysis: high ${:.2} on {}, low ${:.2} on {}.\n",
            high.value, high.date, low.value, low.date
        ));
    }
    p
}

/// EXPECTED PRICES: each prior expected price at its horizon date — once the
/// date has passed (strictly before this run's session, the due rule), the
/// close read and the per-check score, the accuracy pass's own check over a
/// record of the prior position; before it, not reached, the path so far
/// standing under PRICE. Omitted where the prior stated no price.
fn expected_prices_block(input: &ReviewInput, g: &GradedVerdict) -> String {
    let d = input.dossier;
    if prior_prices(g).iter().all(|(_, v)| v.is_none()) {
        return String::new();
    }
    let (Some(created), Some(session)) = (prior_session(d), run_session(d)) else {
        return String::new();
    };
    // The prior position as a price record — the run's spot and its anchor
    // bar, its model prices — so the check reads the close and the bridge
    // exactly as the accuracy pass would.
    let record = PriceRecord {
        symbol: d.position.symbol.clone(),
        created_on: iso(created),
        spot: d.prior_spot.unwrap_or(0.0),
        anchor: d.prior_authoring_close.clone(),
        model: HorizonPrices {
            three_month: g.appendix.expected_price_3m,
            twelve_month: g.appendix.expected_price_12m,
            three_year: g.appendix.expected_price_3y,
        },
        engine: HorizonPrices::default(),
    };
    let mut p = format!(
        "\nEXPECTED PRICES\nEach expected price under PRIOR POSITION at its horizon date: once the \
         date has passed, the close on it — or the last close within {} sessions before it — \
         and the score, 100 × (1 − |expected − close| ÷ close), floored at 0; before it, not \
         reached, the path so far under PRICE.\n",
        outcome::HORIZON_PROXIMITY_SESSIONS
    );
    for (h, price) in prior_prices(g) {
        let Some(_) = price else {
            p.push_str(&format!("- {}: none stated.\n", horizon_words(h)));
            continue;
        };
        let Some(date) = outcome::horizon_date(created, h) else {
            continue;
        };
        if date >= session {
            p.push_str(&format!("- {} ({}): not reached.\n", horizon_words(h), iso(date)));
            continue;
        }
        if input.price_bridge.is_none() {
            p.push_str(&format!("- {} ({}): passed; withheld.\n", horizon_words(h), iso(date)));
            continue;
        }
        match outcome::check_horizon(&record, h, &d.financials.daily_closes) {
            CheckOutcome::Scored { close, model: Some(leg), .. } => p.push_str(&format!(
                "- {} ({}): close ${:.2} on {}; score {:.1}.\n",
                horizon_words(h),
                iso(date),
                close.value,
                close.date,
                leg.score
            )),
            CheckOutcome::Scored { model: None, .. } => {
                p.push_str(&format!("- {}: none stated.\n", horizon_words(h)))
            }
            CheckOutcome::Unscorable { cause } => p.push_str(&format!(
                "- {} ({}): passed, not scored — {}.\n",
                horizon_words(h),
                iso(date),
                cause_words(cause)
            )),
        }
    }
    p
}

/// ACCURACY SCORES and the checks since the prior analysis
/// (`docs/portfolio-analysis.md` §Outcome learning): the six scores, each with
/// the date of the check that last moved it and whether the prior analysis
/// read it; then every check written after the prior pass's mark, newest by
/// horizon date up to the drafted count, with the total where the cap trimmed
/// some — or, when none landed, the one fixed sentence.
fn accuracy_block(d: &HoldingDossier) -> String {
    let Some(acc) = &d.accuracy else {
        return "\nACCURACY SCORES\nThe accuracy record could not be read this run.\n".to_string();
    };
    let mark = d.prior_accuracy_read_through;
    let mut p = String::from(
        "\nACCURACY SCORES\nHow the expected prices stated on this holding have scored once their \
         horizon dates passed, at each horizon, for the analyst's prices and for the computed \
         base values: the mean of the per-check scores, each 100 × (1 − |expected − close| ÷ \
         close), floored at 0, so 0 to 100; no score yet before the first check. Each score names \
         the date of the check that last moved it and whether the prior analysis read it.\n",
    );
    let arm = |s: Option<&outcome::ArmScore>| match s {
        None => "no score yet".to_string(),
        Some(s) => format!(
            "{:.1} over {} check{}, last moved {}, {}",
            s.score,
            s.checks,
            if s.checks == 1 { "" } else { "s" },
            s.last_moved_on,
            if s.read_by(mark) { "read by the prior analysis" } else { "not read by the prior analysis" }
        ),
    };
    for h in Horizon::ALL {
        let at = acc.scores.at(h);
        p.push_str(&format!(
            "- {}: analyst {}; computed base {}.\n",
            horizon_words(h),
            arm(at.model.as_ref()),
            arm(at.engine.as_ref())
        ));
    }
    let since = outcome::checks_since(acc, mark, outcome::REVIEW_CHECK_LINES);
    if since.lines.is_empty() {
        p.push_str(match mark {
            Some(_) => {
                "\nCHECKS SINCE THE PRIOR ANALYSIS\nNo check landed after the prior analysis was \
                 written: the scores above are the ones it read, its thesis document under PRIOR \
                 THESIS.\n"
            }
            // No mark and no line: no check exists on the holding's forecasts.
            None => "\nCHECKS SINCE THE PRIOR ANALYSIS\nNo check has been written on this holding's \
                     forecasts.\n",
        });
        return p;
    }
    // With no mark the prior read no accuracy record: every check is unread,
    // but not every one landed after it (ruled 2026-10-09).
    p.push_str(match mark {
        Some(_) => {
            "\nCHECKS SINCE THE PRIOR ANALYSIS\nEvery line landed after the prior analysis was \
             written; none was read before. One line per forecast and horizon, newest horizon \
             date first: the forecast's date, the horizon and its date, the close read, and each \
             expected price with its score.\n"
        }
        None => {
            "\nCHECKS SINCE THE PRIOR ANALYSIS\nThe prior analysis read no accuracy record, so none \
             of these was read before. One line per forecast and horizon, newest horizon date \
             first: the forecast's date, the horizon and its date, the close read, and each \
             expected price with its score.\n"
        }
    });
    for (ep, check, date) in &since.lines {
        let head = format!(
            "- forecast of {}, {} ({})",
            ep.record.created_on,
            horizon_words(check.check.horizon),
            iso(*date)
        );
        match &check.check.outcome {
            CheckOutcome::Scored { close, bridge_factor, model, engine } => {
                let leg = |l: &Option<outcome::LegScore>| match l {
                    Some(l) => format!(
                        "${:.2}{}, score {:.1}",
                        l.expected,
                        if *bridge_factor != 1.0 { " (bridged)" } else { "" },
                        l.score
                    ),
                    None => "none stated".to_string(),
                };
                p.push_str(&format!(
                    "{head}: close ${:.2} on {}; analyst {}; computed base {}.\n",
                    close.value,
                    close.date,
                    leg(model),
                    leg(engine)
                ));
            }
            CheckOutcome::Unscorable { cause } => {
                p.push_str(&format!("{head}: not scored — {}.\n", cause_words(*cause)));
            }
        }
    }
    if since.total > since.lines.len() {
        p.push_str(&format!(
            "The {} lines with the newest horizon dates of {}.\n",
            since.lines.len(),
            since.total
        ));
    }
    p
}

/// How a then → now value renders: as is, or a fraction as a percentage.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Unit {
    Plain,
    Percent,
}

/// Decimals past the base the shared precision may rise before both sides
/// fall back to the shortest round-trip scientific form.
const PRECISION_CAP: usize = 64;

/// A value at `dec` decimals in its unit. A percentage shifts the decimal
/// point of the raw value's correctly rounded expansion two places rather than
/// multiplying by 100, so the scaling never rounds two distinct values to one.
fn fixed(x: f64, unit: Unit, dec: usize) -> String {
    if !x.is_finite() {
        return match unit {
            Unit::Plain => format!("{x}"),
            Unit::Percent => format!("{x}%"),
        };
    }
    match unit {
        Unit::Plain => format!("{x:.dec$}"),
        Unit::Percent => format!("{}%", shift_point_two(&format!("{:.*}", dec + 2, x))),
    }
}

/// A fixed-point rendering with at least two decimals, its point moved two
/// places right: `-0.0371` → `-3.71`, `1.50` → `150`.
fn shift_point_two(s: &str) -> String {
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s),
    };
    let (int, frac) = digits.split_once('.').expect("a fixed point with two decimals at least");
    let (moved, rest) = frac.split_at(2);
    let whole = format!("{int}{moved}");
    let whole = match whole.trim_start_matches('0') {
        "" => "0",
        w => w,
    };
    if rest.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{rest}")
    }
}

/// The shortest round-trip scientific form — distinct for any two distinct
/// values; a percentage moves the exponent, never the value.
fn scientific(x: f64, unit: Unit) -> String {
    let s = format!("{x:e}");
    match (unit, s.split_once('e')) {
        (Unit::Percent, Some((mantissa, exp))) => match exp.parse::<i32>() {
            Ok(exp) => format!("{mantissa}e{}%", exp + 2),
            Err(_) => format!("{s}%"),
        },
        (Unit::Percent, None) => format!("{s}%"),
        (Unit::Plain, _) => s,
    }
}

/// The shared precision of then → now pairs (`docs/portfolio-workflow.md`
/// §Step 6b): the base decimals, raised until every pair whose values differ
/// renders differently, so a real move never reads as equality; `None` past
/// [`PRECISION_CAP`], where the scientific form takes over.
fn distinct_decimals(pairs: &[(f64, f64)], unit: Unit, decimals: usize) -> Option<usize> {
    (decimals..=decimals + PRECISION_CAP).find(|&dec| {
        pairs
            .iter()
            .all(|(a, b)| a == b || fixed(*a, unit, dec) != fixed(*b, unit, dec))
    })
}

/// The renderer for one set of then → now pairs: the shared precision, or the
/// scientific form past the cap.
fn renderer(pairs: &[(f64, f64)], unit: Unit, decimals: usize) -> impl Fn(f64) -> String {
    let dec = distinct_decimals(pairs, unit, decimals);
    move |x| match dec {
        Some(d) => fixed(x, unit, d),
        None => scientific(x, unit),
    }
}

/// A then → now pair at the shared precision ([`renderer`]).
fn then_now(then: Option<f64>, now: Option<f64>, unit: Unit, decimals: usize) -> String {
    let pairs: Vec<(f64, f64)> = then.zip(now).into_iter().collect();
    let render = renderer(&pairs, unit, decimals);
    let side = |v: Option<f64>| v.map(&render).unwrap_or_else(|| "(gap)".to_string());
    format!("{} → {}", side(then), side(now))
}

/// The metric's rendering: a fraction as a percentage, a multiple as is.
fn metric_unit(name: &str) -> (Unit, usize) {
    match name {
        "debt/equity" | "P/E" | "P/S" | "P/B" => (Unit::Plain, 2),
        _ => (Unit::Percent, 1),
    }
}

/// COMPUTED READS: the engine's own grade, scores, risk tier, capital-efficiency
/// read, price bands and metrics at the prior analysis beside this run's — the
/// prior bands on today's basis through the bridge, withheld where it was
/// unresolvable — and any method change named for what it changed on the
/// prior's own branch, absent where it changed nothing.
fn computed_then_now_block(input: &ReviewInput, g: &GradedVerdict, now: &EngineOutput) -> String {
    let d = input.dossier;
    let branch = if g.fund_class_label.is_some() {
        engine::GradeBranch::Fund
    } else {
        engine::GradeBranch::Stock
    };
    let realized = engine::realized_engine_data(
        &engine::PriorEngineRead {
            verdict: g,
            metrics: d.prior_metrics.as_ref(),
            grade_parameter_version: d.prior_grade_parameter_version.as_deref(),
            target_parameter_version: d.prior_target_parameter_version.as_deref(),
            branch,
        },
        now,
        price_now(d),
        move_since_prior(d),
        input.price_bridge,
    );
    let mut p = String::from(
        "\nCOMPUTED READS\nThe computed reads at the prior analysis beside this run's, then → now; \
         the prior price bands on today's price basis.\n",
    );
    p.push_str(&format!("- grade: {} → {}.\n", realized.grade.then.as_str(), realized.grade.now.as_str()));
    let scores: Vec<String> = realized
        .sub_scores
        .iter()
        .map(|(name, pair)| format!("{name} {}", then_now(Some(pair.then), Some(pair.now), Unit::Plain, 0)))
        .collect();
    p.push_str(&format!(
        "- scores (0 to 100, higher is better; valuation higher means more attractive, risk \
         higher means more resilient): {}.\n",
        scores.join("; ")
    ));
    p.push_str(&format!("- risk tier: {} → {}.\n", realized.tier.then.as_str(), realized.tier.now.as_str()));
    p.push_str(&format!(
        "- capital efficiency: {} → {}.\n",
        hurdle_words(realized.hurdle.then),
        hurdle_words(realized.hurdle.now)
    ));
    let marked = input.price_bridge.is_some_and(|f| f != 1.0);
    let bands: Vec<String> = realized
        .bands
        .iter()
        .filter(|(_, pair)| pair.then.is_some() || pair.now.is_some())
        .map(|(name, pair): (&str, &ThenNow<Option<engine::BandPrices>>)| {
            // One precision for the whole triple pair: each leg's then and now
            // stay distinguishable wherever they differ.
            let legs = |b: &engine::BandPrices| [b.bear, b.base, b.bull];
            let pairs: Vec<(f64, f64)> = match (pair.then, pair.now) {
                (Some(t), Some(n)) => legs(&t).into_iter().zip(legs(&n)).collect(),
                _ => Vec::new(),
            };
            let render = renderer(&pairs, Unit::Plain, 2);
            let band = |b: Option<engine::BandPrices>| match b {
                Some(b) => format!("{} / {} / {}", render(b.bear), render(b.base), render(b.bull)),
                None => "(gap)".to_string(),
            };
            let then = match (pair.then, input.price_bridge) {
                (Some(b), _) => format!("{}{}", band(Some(b)), if marked { " (bridged)" } else { "" }),
                (None, None) if prior_band(g, name).is_some() => "withheld".to_string(),
                (None, _) => band(None),
            };
            format!("{name} {then} → {}", band(pair.now))
        })
        .collect();
    if !bands.is_empty() {
        p.push_str(&format!("- price bands (USD, bear / base / bull): {}.\n", bands.join("; ")));
    }
    let metrics: Vec<String> = realized
        .metrics
        .iter()
        .filter(|(_, pair)| pair.then.is_some() || pair.now.is_some())
        .map(|(name, pair)| {
            let (unit, dec) = metric_unit(name);
            format!("{name} {}", then_now(pair.then, pair.now, unit, dec))
        })
        .collect();
    if !metrics.is_empty() {
        p.push_str(&format!("- metrics: {}.\n", metrics.join("; ")));
    }
    if let Some(change) = realized.grade_boundary {
        p.push_str(grade_boundary_line(change));
    }
    if let Some(horizons) = realized.target_boundary {
        let named = horizons.label().replace("target", "price band");
        p.push_str(&format!(
            "Method change: the method behind the {named} changed after the prior analysis, so \
             a move there can come from the method with no change in the inputs.\n"
        ));
    }
    p
}

/// Whether the prior verdict carried a band at the named horizon.
fn prior_band<'a>(g: &'a GradedVerdict, name: &str) -> Option<&'a crate::portfolio::PriceTarget> {
    match name {
        "three-month" => g.price_targets.three_month.as_ref(),
        "twelve-month" => g.price_targets.twelve_month.as_ref(),
        _ => g.price_targets.three_year.as_ref(),
    }
}

/// The grade-parameter boundary named for what it changed
/// (`docs/portfolio-analysis.md` §Starting parameters): a band
/// recalibration, the fund momentum re-homing, or the fund sector-P/E
/// exchange-basis correction.
fn grade_boundary_line(change: engine::GradeParameterChange) -> &'static str {
    match change {
        engine::GradeParameterChange::Letters => {
            "Method change: the scoring bands behind the grade were recalibrated after the prior \
             analysis, so the grade and the scores can move with no change in the inputs.\n"
        }
        engine::GradeParameterChange::FundMomentum => {
            "Method change: the price window behind the fund's momentum score changed after the \
             prior analysis, so the momentum score can move with no change in the inputs.\n"
        }
        engine::GradeParameterChange::FundSectorPeBasis => {
            "Method change: the fund's valuation score now reads sector P/E figures only where \
             both exchanges serve them, so the valuation score and the grade can move with no \
             change in the inputs.\n"
        }
    }
}

/// The `role_risk_only` REALIZED block (`docs/portfolio-workflow.md` §Step
/// 6e): the price and the NAV then and now, and the exposure and expense reads
/// then and now — no accuracy scores, since the branch states no price.
fn role_risk_realized(input: &ReviewInput) -> String {
    let d = input.dossier;
    let then = d.prior_fund_exposure.as_ref();
    let now = d.fund.as_ref().map(|f| crate::portfolio::fund::exposure_basis(&f.fund));
    let mut p = format!(
        "\nREALIZED\nWhat has happened since the prior analysis, computed from the fetched prices \
         and the stored reads, then → now.{}\n",
        basis_gloss(input.price_bridge)
    );
    let price_then = d
        .prior_authoring_close
        .as_ref()
        .map(|a| match bridged(a.value, input.price_bridge) {
            Some(v) => format!("{} (the close on {})", money_marked(Some(v)), a.date),
            None => "withheld".to_string(),
        })
        .unwrap_or_else(|| "(gap)".into());
    let price_now_s = price_now(d).map(|v| format!("${v:.2}")).unwrap_or_else(|| "(gap)".into());
    let moved = move_since_prior(d)
        .map(|m| format!("; {:+.1}% since the prior analysis", m * 100.0))
        .unwrap_or_default();
    p.push_str(&format!("- price per share: {price_then} → {price_now_s}{moved}.\n"));
    let nav_then = then
        .and_then(|b| b.nav)
        .map(|v| money_marked(bridged(v, input.price_bridge)))
        .unwrap_or_else(|| "(gap)".into());
    let nav_now = now
        .as_ref()
        .and_then(|b| b.nav)
        .map(|v| format!("${v:.2}"))
        .unwrap_or_else(|| "(gap)".into());
    p.push_str(&format!("- NAV per share: {nav_then} → {nav_now}.\n"));
    p.push_str(&format!(
        "- expense ratio: {}.\n",
        then_now(
            then.and_then(|b| b.expense_ratio),
            now.as_ref().and_then(|b| b.expense_ratio),
            Unit::Percent,
            2
        )
    ));
    let text = |v: Option<&str>| v.unwrap_or("(gap)").to_string();
    p.push_str(&format!(
        "- class: {} → {}.\n",
        text(then.map(|b| b.class_label.as_str())),
        text(now.as_ref().map(|b| b.class_label.as_str()))
    ));
    let us_then = then.and_then(|b| b.us_share);
    let us_now = now.as_ref().and_then(|b| b.us_share);
    if us_then.is_some() || us_now.is_some() {
        p.push_str(&format!("- US share of holdings: {}.\n", then_now(us_then, us_now, Unit::Percent, 0)));
    }
    let sector = |b: Option<&crate::portfolio::fund::FundExposureBasis>| {
        b.and_then(|b| b.top_sector.as_ref())
            .map(|(label, w)| format!("{label} {:.1}%", w * 100.0))
            .unwrap_or_else(|| "(gap)".into())
    };
    if then.and_then(|b| b.top_sector.as_ref()).is_some()
        || now.as_ref().and_then(|b| b.top_sector.as_ref()).is_some()
    {
        p.push_str(&format!("- largest sector: {} → {}.\n", sector(then), sector(now.as_ref())));
    }
    let flag = |b: Option<&crate::portfolio::fund::FundExposureBasis>| match b {
        Some(b) if b.structural_flag => "yes",
        Some(_) => "no",
        None => "(gap)",
    };
    p.push_str(&format!(
        "- leveraged, inverse or option-overlay structure: {} → {}.\n",
        flag(then),
        flag(now.as_ref())
    ));
    p
}

// ---- PART 2 ------------------------------------------------------------------

/// Part 2: the review's items in output order, each naming the Part 1 section
/// it draws on — each expected price against what happened, each falsifier
/// and trigger tripped or fired or not, whether the thesis survives, where the
/// prior read was right or wrong and why, what to revise, and what should
/// change in how the holding is analyzed — within the review's length band.
fn task_section(input: &ReviewInput, position: &PriorPosition) -> String {
    let role = position.role_read || matches!(input.subject, ReviewSubject::RoleRisk);
    let mut p = String::from(PART_2);
    p.push_str(
        "Write the review of the prior position as plain text — no code fence, no JSON, no \
         heading before the first line. It covers, in this order:\n",
    );
    let first = if position.has_prices {
        "Each expected price under PRIOR POSITION against what happened, from REALIZED."
    } else if position.role_read {
        "The action under PRIOR POSITION against what happened, from REALIZED."
    } else if role {
        // A role read states no price: after an abstention the document
        // itself is what the numbers test.
        "The document under PRIOR THESIS against what happened, from PRIOR POSITION and \
         REALIZED."
    } else {
        "Each expected price the document under PRIOR THESIS states against what happened, \
         from PRIOR POSITION and REALIZED."
    };
    p.push_str(&format!("\n1. {first}\n"));
    if role {
        p.push_str(
            "\n2. Each trigger the document under PRIOR THESIS names — whether it fired, by the \
             numbers under FETCHED VALUES, ANALYSIS and REALIZED.\n",
        );
        p.push_str("\n3. Whether the role read survives.\n");
    } else {
        p.push_str(
            "\n2. Each falsifier and each trigger the document under PRIOR THESIS names — whether \
             the falsifier tripped or the trigger fired, by the numbers under FETCHED VALUES, \
             ANALYSIS and REALIZED.\n",
        );
        p.push_str("\n3. Whether the thesis survives.\n");
    }
    p.push_str("\n4. Where the prior read was right or wrong, and why.\n");
    p.push_str("\n5. What to revise.\n");
    p.push_str("\n6. What should change in how this holding is analyzed.\n");
    p.push_str(&format!(
        "\nThe review runs {} to {} words.\n",
        REVIEW_WORDS.0, REVIEW_WORDS.1
    ));
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::engine::EngineVerdict;
    use crate::portfolio::outcome::{Check, LegScore, StoredCheck, StoredEpisode};
    use crate::portfolio::pipeline::tests::{dossier, rates, strong_financials};
    use crate::portfolio::pipeline::{analyze_holding, StubAnalyst};
    use crate::portfolio::{AssetClass, HoldingVerdict};

    fn bar(date: &str, value: f64) -> DatedValue {
        DatedValue { date: date.into(), value }
    }

    /// A continuity dossier: the stub's own first pass on 2026-07-01 as the
    /// prior, its spot, anchor and stamps loaded, the run on 2026-10-09.
    fn continuity() -> (HoldingDossier, EngineOutput) {
        let mut d = dossier(AssetClass::Stock, strong_financials());
        let (prior, audit) = analyze_holding(&StubAnalyst, &d, &rates(), "2026-07-01").unwrap();
        let now = match engine::analyze(&d.financials, &rates()) {
            EngineVerdict::Analyzed(o) => o,
            other => panic!("{other:?}"),
        };
        // The review's own reads off this run's series: the price now, the
        // close the move is measured from, the path since and the horizon
        // closes.
        d.financials.daily_closes = vec![
            bar("2026-06-30", 100.0),
            bar("2026-07-15", 120.0),
            bar("2026-08-14", 90.0),
            bar("2026-10-01", 110.0),
            bar("2026-10-08", 112.0),
        ];
        d.financials.current_price = Some(115.0);
        d.prior_verdict = Some(HoldingVerdict { analyzed_at: Some("2026-07-01T14:00:00Z".into()), ..prior });
        d.prior_vintage = Some("2026-07-01T14:00:00Z".into());
        d.prior_spot = Some(100.0);
        d.prior_authoring_close = Some(bar("2026-06-30", 100.0));
        d.prior_metrics = Some(audit.metrics.clone());
        d.prior_grade_parameter_version = Some(engine::GRADE_PARAMETER_VERSION.into());
        d.prior_target_parameter_version = Some(engine::SCENARIO_TARGET_PARAMETER_VERSION.into());
        d.analysis_date = "2026-10-09".into();
        graded_mut(&mut d).action_rationale = "Holds its place on the margin trajectory.".into();
        (d, *now)
    }

    fn graded_mut(d: &mut HoldingDossier) -> &mut GradedVerdict {
        match &mut d.prior_verdict.as_mut().unwrap().disposition {
            VerdictDisposition::Priced(g) => g,
            other => panic!("{other:?}"),
        }
    }

    fn analysis() -> AnalysisRecord {
        AnalysisRecord { text: "This run's analysis.".into(), written: "2026-10-09".into(), anchor: None }
    }

    fn render(d: &HoldingDossier, now: &EngineOutput, bridge: Option<f64>) -> String {
        let a = analysis();
        review_prompt(&ReviewInput {
            dossier: d,
            rates: &rates(),
            analysis: &a,
            prior_split: None,
            price_bridge: bridge,
            subject: ReviewSubject::Priced { engine: now },
        })
        .user
    }

    fn check(id: i64, episode_id: i64, horizon: Horizon, on: &str, model: f64, engine: f64) -> StoredCheck {
        StoredCheck {
            id,
            episode_id,
            check: Check {
                horizon,
                checked_on: on.into(),
                run_id: "run".into(),
                outcome: CheckOutcome::Scored {
                    close: bar(on, 110.0),
                    bridge_factor: 1.0,
                    model: Some(LegScore { expected: 105.0, score: model }),
                    engine: Some(LegScore { expected: 100.0, score: engine }),
                },
            },
        }
    }

    fn episode(id: i64, created_on: &str) -> StoredEpisode {
        StoredEpisode {
            id,
            record: PriceRecord {
                symbol: "AAPL".into(),
                created_on: created_on.into(),
                spot: 100.0,
                anchor: Some(bar("2026-06-30", 100.0)),
                model: HorizonPrices { three_month: Some(105.0), ..Default::default() },
                engine: HorizonPrices { three_month: Some(100.0), ..Default::default() },
            },
        }
    }

    #[test]
    fn the_message_carries_the_sections_in_page_order_and_part_2_names_each() {
        let (mut d, now) = continuity();
        d.accuracy = Some(outcome::subject_accuracy(&[], &[], "AAPL"));
        let user = render(&d, &now, Some(1.0));
        let order = [
            "======== PART 1: INPUTS ========",
            "\nHOLDING\n",
            "\nFETCHED VALUES",
            "\nPRIOR POSITION\n",
            "\nPRIOR THESIS (written 2026-07-01)\n",
            "\nANALYSIS\n",
            "\nREALIZED\n",
            "\nPRICE\n",
            "\nEXPECTED PRICES\n",
            "\nACCURACY SCORES\n",
            "\nCHECKS SINCE THE PRIOR ANALYSIS\n",
            "\nCOMPUTED READS\n",
            "======== PART 2: TASK ========",
        ];
        let mut at = 0;
        for heading in order {
            let found = user[at..].find(heading).unwrap_or_else(|| panic!("{heading} out of order:\n{user}"));
            at += found + heading.len();
        }
        let part2 = &user[user.find("PART 2").unwrap()..];
        for named in ["PRIOR POSITION", "PRIOR THESIS", "FETCHED VALUES", "ANALYSIS", "REALIZED"] {
            assert!(part2.contains(named), "Part 2 never names {named}:\n{part2}");
        }
        assert!(part2.contains("The review runs 400 to 900 words."));
        // No app concept reaches the model.
        for banned in ["engine", "episode", "self-review", "arm", "pipeline", "run id"] {
            assert!(!user.to_lowercase().contains(&format!(" {banned} ")), "{banned}:\n{user}");
        }
    }

    #[test]
    fn prior_position_dates_each_price_and_marks_the_bridge() {
        let (mut d, now) = continuity();
        graded_mut(&mut d).appendix.expected_price_12m = None;
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains("Date: 2026-07-01.\nPrice then: $100.00 per share.\n"), "{user}");
        assert!(user.contains("Action: "), "{user}");
        assert!(user.contains("by 2026-10-01.\n"), "{user}");
        assert!(user.contains("Expected price at twelve months: none stated (2027-07-01).\n"), "{user}");
        assert!(!user.contains("bridged"), "{user}");
        // A 2:1 split since: every prior price is halved and marked, the gloss says so.
        let split = render(&d, &now, Some(0.5));
        assert!(split.contains("Price then: $50.00 (bridged) per share."), "{split}");
        assert!(split.contains("A price marked bridged was converted"), "{split}");
        // An unresolvable bridge withholds every prior price and says why.
        let withheld = render(&d, &now, None);
        assert!(withheld.contains("Price then: withheld.\n"), "{withheld}");
        assert!(withheld.contains("could not be verified this run"), "{withheld}");
        assert!(withheld.contains("passed; withheld."), "{withheld}");
    }

    #[test]
    fn a_rule_demoted_prior_renders_as_set_by_rule_with_no_rationale() {
        let (mut d, now) = continuity();
        d.prior_verdict.as_mut().unwrap().action_source = ActionSource::RuleDemoted;
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains(", set by rule after the prior analysis, not chosen in it.\n"), "{user}");
        let position = &user[user.find("PRIOR POSITION").unwrap()..user.find("PRIOR THESIS").unwrap()];
        assert!(!position.contains("Rationale:"), "{position}");
    }

    #[test]
    fn a_passed_horizon_scores_its_close_and_an_open_one_shows_the_path() {
        let (mut d, now) = continuity();
        graded_mut(&mut d).appendix.expected_price_3m = Some(100.0);
        let user = render(&d, &now, Some(1.0));
        // The three-month date 2026-10-01 is before the run's session: the
        // close on it scores the prior price (100 vs 110: 90.9).
        assert!(user.contains("- three months (2026-10-01): close $110.00 on 2026-10-01; score 90.9.\n"), "{user}");
        assert!(user.contains("- twelve months (2027-07-01): not reached.\n"), "{user}");
        // The path so far: the highest and lowest closes since the prior session.
        assert!(user.contains("high $120.00 on 2026-07-15, low $90.00 on 2026-08-14."), "{user}");
        // The move since: spot over the newest close before the prior session.
        assert!(user.contains("Now: $115.00 per share, +15.0% since the prior analysis (from the close of $100.00 on 2026-06-30)."), "{user}");
        // A passed horizon whose close is out of reach is written unscorable.
        let mut gap = d.clone();
        gap.financials.daily_closes.retain(|c| c.date.as_str() < "2026-09-01" );
        let user = render(&gap, &now, Some(1.0));
        assert!(user.contains("passed, not scored — no close within 5 sessions before the date."), "{user}");
    }

    #[test]
    fn the_accuracy_record_reads_the_prior_mark_lists_new_checks_and_caps_them() {
        let (mut d, now) = continuity();
        let eps = vec![episode(1, "2026-04-01"), episode(2, "2026-05-01")];
        let checks = vec![
            check(10, 1, Horizon::ThreeMonth, "2026-07-02", 80.0, 70.0),
            check(11, 2, Horizon::ThreeMonth, "2026-08-03", 60.0, 50.0),
        ];
        d.accuracy = Some(outcome::subject_accuracy(&eps, &checks, "AAPL"));
        // The prior read through check 10: check 11 moved the scores after it.
        d.prior_accuracy_read_through = Some(10);
        let user = render(&d, &now, Some(1.0));
        assert!(
            user.contains("- three months: analyst 70.0 over 2 checks, last moved 2026-08-03, not read by the prior analysis;"),
            "{user}"
        );
        assert!(user.contains("- twelve months: analyst no score yet; computed base no score yet.\n"), "{user}");
        assert!(user.contains("Every line landed after the prior analysis was written; none was read before."), "{user}");
        assert!(
            user.contains("- forecast of 2026-05-01, three months (2026-08-01): close $110.00 on 2026-08-03; analyst $105.00, score 60.0; computed base $100.00, score 50.0.\n"),
            "{user}"
        );
        assert!(!user.contains("forecast of 2026-04-01"), "check 10 was read:\n{user}");
        // Read through everything: the scores read as read and the fixed sentence stands.
        d.prior_accuracy_read_through = Some(11);
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains("last moved 2026-08-03, read by the prior analysis"), "{user}");
        assert!(user.contains("No check landed after the prior analysis was written: the scores above are the ones it read, its thesis document under PRIOR THESIS."), "{user}");
        // Past the cap: the newest twelve with the total.
        let eps: Vec<StoredEpisode> = (1..=14).map(|i| episode(i, &format!("2025-{:02}-01", (i % 12) + 1))).collect();
        let checks: Vec<StoredCheck> = (1..=14).map(|i| check(100 + i, i, Horizon::ThreeMonth, "2026-01-02", 50.0, 50.0)).collect();
        d.accuracy = Some(outcome::subject_accuracy(&eps, &checks, "AAPL"));
        d.prior_accuracy_read_through = None;
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains("The 12 lines with the newest horizon dates of 14.\n"), "{user}");
        // An unreadable store says so.
        d.accuracy = None;
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains("ACCURACY SCORES\nThe accuracy record could not be read this run.\n"), "{user}");
        assert!(!user.contains("CHECKS SINCE"), "{user}");
    }

    #[test]
    fn an_unscorable_check_renders_its_cause() {
        let (mut d, now) = continuity();
        let mut c = check(5, 1, Horizon::ThreeMonth, "2026-07-02", 0.0, 0.0);
        c.check.outcome = CheckOutcome::Unscorable { cause: UnscorableCause::NoCloseInProximity };
        d.accuracy = Some(outcome::subject_accuracy(&[episode(1, "2026-04-01")], &[c], "AAPL"));
        let user = render(&d, &now, Some(1.0));
        assert!(
            user.contains("- forecast of 2026-04-01, three months (2026-07-01): not scored — no close within 5 sessions before the date.\n"),
            "{user}"
        );
    }

    #[test]
    fn the_computed_reads_pair_then_and_now_and_name_a_method_change() {
        let (mut d, now) = continuity();
        let user = render(&d, &now, Some(1.0));
        let block = &user[user.find("COMPUTED READS").unwrap()..user.find("PART 2").unwrap()];
        assert!(block.contains("- grade: "), "{block}");
        assert!(block.contains("- risk tier: "), "{block}");
        assert!(block.contains("- capital efficiency: "), "{block}");
        assert!(block.contains("- price bands (USD, bear / base / bull): three-month "), "{block}");
        assert!(!block.contains("Method change"), "{block}");
        // A sub-precision move stays distinguishable.
        assert_eq!(then_now(Some(70.0), Some(70.0 + 1e-6), Unit::Plain, 0), "70.000000 → 70.000001");
        assert_eq!(then_now(Some(70.0), Some(70.0), Unit::Plain, 0), "70 → 70");
        // A prior on an older target stamp names the bands its boundary moved.
        d.prior_target_parameter_version = Some("targets-v6".into());
        let user = render(&d, &now, Some(1.0));
        assert!(
            user.contains("Method change: the method behind the three-month and three-year price bands changed after the prior analysis"),
            "{user}"
        );
        // A prior grade stamp across the letter recalibration names it.
        d.prior_grade_parameter_version = Some("grade-v2".into());
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains("Method change: the scoring bands behind the grade were recalibrated"), "{user}");
        // An unresolvable bridge withholds the prior bands.
        let user = render(&d, &now, None);
        assert!(user.contains("three-month withheld → "), "{user}");
    }

    #[test]
    fn an_abstained_prior_reviews_what_its_row_retains() {
        let (mut d, now) = continuity();
        let doc = d.prior_verdict.as_ref().unwrap().thesis_document().unwrap().to_string();
        d.prior_verdict.as_mut().unwrap().disposition = VerdictDisposition::InsufficientEvidence {
            reason: "fund metadata unavailable".into(),
            prior_thesis_document: Some(doc),
        };
        d.prior_spot = None;
        d.prior_metrics = None;
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains("Price then: $100.00 per share (the close on 2026-06-30).\n"), "{user}");
        assert!(user.contains("A later analysis made no call, so no action, conviction or expected price is on record"), "{user}");
        // The abstention's own reason is app text and never reaches the model.
        assert!(!user.contains("fund metadata unavailable"), "{user}");
        assert!(!user.contains("EXPECTED PRICES"), "{user}");
        assert!(!user.contains("COMPUTED READS"), "{user}");
        assert!(user.contains("ACCURACY SCORES"), "{user}");
        assert!(user.contains("1. Each expected price the document under PRIOR THESIS states"), "{user}");
    }

    #[test]
    fn a_role_risk_review_reads_the_prior_action_and_the_funds_realized_data() {
        use crate::portfolio::fund::FundExposureBasis;
        let (mut d, _) = continuity();
        d.prior_verdict.as_mut().unwrap().disposition =
            VerdictDisposition::RoleRiskOnly(Box::new(crate::portfolio::RoleRiskVerdict {
                class_label: "bond fund".into(),
                thesis_document: "The role read.".into(),
                exposure_tilt: vec![],
                expense_drag: None,
                observable_risk: None,
                structural_flag: false,
                is_cef: false,
                nav_premium: None,
                evidence_gaps: vec![],
                action: crate::portfolio::Action::Hold,
                action_rationale: "Keeps its ballast role.".into(),
            }));
        d.prior_fund_exposure = Some(FundExposureBasis {
            class_label: "bond fund".into(),
            expense_ratio: Some(0.0003),
            us_share: None,
            top_sector: None,
            structural_flag: false,
            nav: Some(72.10),
        });
        let a = analysis();
        let user = review_prompt(&ReviewInput {
            dossier: &d,
            rates: &rates(),
            analysis: &a,
            prior_split: None,
            price_bridge: Some(1.0),
            subject: ReviewSubject::RoleRisk,
        })
        .user;
        let position = &user[user.find("PRIOR POSITION").unwrap()..user.find("PRIOR THESIS").unwrap()];
        assert!(position.contains("Action: hold, chosen in the prior analysis.\nRationale: Keeps its ballast role.\n"), "{position}");
        assert!(!position.contains("Date:") && !position.contains("Expected price"), "{position}");
        assert!(user.contains("- price per share: $100.00 (the close on 2026-06-30) → $115.00; +15.0% since the prior analysis.\n"), "{user}");
        assert!(user.contains("- NAV per share: $72.10 → (gap).\n"), "{user}");
        assert!(user.contains("- expense ratio: 0.03% → (gap).\n"), "{user}");
        assert!(!user.contains("ACCURACY SCORES"), "{user}");
        assert!(user.contains("1. The action under PRIOR POSITION against what happened, from REALIZED."), "{user}");
        assert!(user.contains("3. Whether the role read survives."), "{user}");
    }

    #[test]
    fn a_band_pair_renders_at_a_shared_precision_that_keeps_a_move_visible() {
        let (mut d, mut now) = continuity();
        // A sub-cent move on one leg of this run's three-month band: the whole
        // triple pair renders at the precision that keeps it distinguishable.
        let prior = graded_mut(&mut d).price_targets.three_month.clone().unwrap();
        now.price_targets.three_month = Some(crate::portfolio::PriceTarget {
            base: prior.base + 0.001,
            ..prior.clone()
        });
        let user = render(&d, &now, Some(1.0));
        let bands = user.lines().find(|l| l.starts_with("- price bands")).unwrap().to_string();
        let three = bands.split("; ").next().unwrap();
        let (then, now_side) = three.split_once(" → ").unwrap();
        assert_ne!(then.trim_start_matches("- price bands (USD, bear / base / bull): three-month "), now_side, "{bands}");
        assert!(then.contains(&format!("{:.3}", prior.base)), "{bands}");
        assert_eq!(distinct_decimals(&[(1.0, 1.0)], Unit::Plain, 2), Some(2));
        assert_eq!(distinct_decimals(&[(1.0, 1.001)], Unit::Plain, 2), Some(3));
    }

    #[test]
    fn a_real_move_never_renders_as_equality_at_any_magnitude() {
        let sides = |s: String| {
            let (then, now) = s.split_once(" → ").unwrap();
            (then.to_string(), now.to_string())
        };
        // Codex's pair: a move far past the old twelve-decimal cap.
        let (then, now) = sides(then_now(Some(50.0), Some(50.00000000000001), Unit::Plain, 0));
        assert_ne!(then, now);
        // A percentage one ulp apart: scaling by 100 could round the two to
        // one product; the shifted point keeps them apart.
        let x = 0.1_f64;
        let y = f64::from_bits(x.to_bits() + 1);
        let (then, now) = sides(then_now(Some(x), Some(y), Unit::Percent, 1));
        assert_ne!(then, now);
        assert!(then.starts_with("10.0") && then.ends_with('%'), "{then}");
        // Past the cap the scientific form takes over, still distinct.
        assert_eq!(then_now(Some(0.0), Some(1e-80), Unit::Plain, 2), "0e0 → 1e-80");
        assert_eq!(then_now(Some(0.0), Some(1e-80), Unit::Percent, 1), "0e2% → 1e-78%");
        // The shifted point renders a percentage as multiplying would.
        assert_eq!(fixed(0.0371, Unit::Percent, 1), "3.7%");
        assert_eq!(fixed(-0.0371, Unit::Percent, 2), "-3.71%");
        assert_eq!(fixed(1.5, Unit::Percent, 0), "150%");
        assert_eq!(fixed(0.0003, Unit::Percent, 2), "0.03%");
        assert_eq!(fixed(f64::INFINITY, Unit::Percent, 1), "inf%");
    }

    #[test]
    fn an_unmarked_prior_lists_every_check_without_claiming_it_landed_after() {
        let (mut d, now) = continuity();
        let checks = vec![check(10, 1, Horizon::ThreeMonth, "2026-07-02", 80.0, 70.0)];
        d.accuracy = Some(outcome::subject_accuracy(&[episode(1, "2026-04-01")], &checks, "AAPL"));
        d.prior_accuracy_read_through = None;
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains("The prior analysis read no accuracy record, so none of these was read before."), "{user}");
        assert!(!user.contains("Every line landed after"), "{user}");
        // No mark and no check at all: the no-check sentence, never the
        // read-scores one.
        d.accuracy = Some(outcome::subject_accuracy(&[], &[], "AAPL"));
        let user = render(&d, &now, Some(1.0));
        assert!(user.contains("\nCHECKS SINCE THE PRIOR ANALYSIS\nNo check has been written on this holding's forecasts.\n"), "{user}");
        assert!(!user.contains("the scores above are the ones it read"), "{user}");
    }

    #[test]
    fn a_role_risk_review_glosses_the_bridge_and_a_role_read_abstention_points_at_the_document() {
        let (mut d, _) = continuity();
        let doc = d.prior_verdict.as_ref().unwrap().thesis_document().unwrap().to_string();
        d.prior_verdict.as_mut().unwrap().disposition = VerdictDisposition::InsufficientEvidence {
            reason: "fund metadata (etf/info) unavailable".into(),
            prior_thesis_document: Some(doc),
        };
        let a = analysis();
        let user = review_prompt(&ReviewInput {
            dossier: &d,
            rates: &rates(),
            analysis: &a,
            prior_split: None,
            price_bridge: Some(0.5),
            subject: ReviewSubject::RoleRisk,
        })
        .user;
        assert!(user.contains("A price marked bridged was converted onto today's price basis"), "{user}");
        let realized = &user[user.find("\nREALIZED\n").unwrap()..];
        assert!(realized.contains("A price marked bridged"), "{realized}");
        assert!(user.contains("1. The document under PRIOR THESIS against what happened, from PRIOR POSITION and REALIZED."), "{user}");
        assert!(!user.contains("etf/info"), "{user}");
        let input = ReviewInput {
            dossier: &d,
            rates: &rates(),
            analysis: &a,
            prior_split: None,
            price_bridge: Some(1.0),
            subject: ReviewSubject::RoleRisk,
        };
        assert!(stub_review(&input).starts_with("[stub: the review of the prior action on AAPL — each trigger"));
    }

    #[test]
    fn the_stub_review_is_deterministic_and_stubbed() {
        let (d, now) = continuity();
        let a = analysis();
        let input = ReviewInput {
            dossier: &d,
            rates: &rates(),
            analysis: &a,
            prior_split: None,
            price_bridge: Some(1.0),
            subject: ReviewSubject::Priced { engine: &now },
        };
        assert_eq!(input.stage(), "review AAPL");
        assert!(stub_review(&input).starts_with("[stub: the review of the prior position on AAPL"));
    }
}
