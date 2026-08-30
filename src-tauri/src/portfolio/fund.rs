//! The fund path (`docs/portfolio-analysis.md` §Asset eligibility): strategy
//! classification at loop time from `etf/info` + weightings, the reduced fund
//! computation — expense drag, exposure tilt, and the **exposure-priced valuation**
//! (a covered-weight-renormalized harmonic composite over the per-sector aggregate
//! P/E, read against its own constant-current-mix history) — and the **fund-form v2
//! scenario targets** (the settled fund-form bullet in §Starting parameters: the
//! shared spread-anchored core over the composite, driver flat, distributions in the
//! total return). Every class the pipeline is structurally unable to price returns
//! the typed `role_risk_only` readout; genuinely missing data abstains under the
//! evidence floor's fund analog. The classification is deterministic and a class the
//! engine can't price honestly degrades — never a fabricated number.

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::portfolio::engine::{
    self, AnchorObservation, CompanyFinancials, ComputedMetrics, DatedValue, EngineOutput,
    RateAnchors, TargetMeta,
};
use crate::portfolio::SubScores;

// ---- Calibration surface (drafted starting values, shadow-tuned) ---------------

/// An equity fund's exposure must be substantially in the composite's market for the
/// exposure-priced valuation to be an honest read (`docs/portfolio-analysis.md`
/// §Asset eligibility, drafted ≥ 70% US by country weightings).
pub const US_EXPOSURE_GUARD: f64 = 0.70;

/// Minimum share of fund weight in P/E-usable sectors below which the valuation is
/// recorded as a gap rather than lettered off a sliver (drafted ≥ 70%).
pub const PE_COVERAGE_GUARD: f64 = 0.70;

/// Minimum constant-mix history samples for the vs-own-history valuation read and
/// the fund-form anchor window (mirrors the stock function's observation floor).
const MIN_COMPOSITE_HISTORY: usize = 8;

/// How many quarterly samples the constant-mix history takes (the shared ~12-quarter
/// anchor window).
const HISTORY_SAMPLE_QUARTERS: usize = 12;

/// Country labels counted as US exposure in `etf/country-weightings` payloads.
const US_LABELS: &[&str] = &["united states", "united states of america", "usa", "u.s.", "us"];

/// Name / mandate fragments that deterministically flag a structurally
/// path-dependent vehicle (leveraged / inverse daily-reset products) — the same
/// screen the report's movers list applies. A leveraged / inverse match routes the
/// fund to `role_risk_only`.
const STRUCTURAL_FLAG_FRAGMENTS: &[&str] = &[
    "2x", "3x", "-1x", "-2x", "-3x", "inverse", "leveraged", "daily bear", "daily bull",
];

/// Duration PHRASES that disqualify the ambiguous fragments ("short", "ultra")
/// from reading as an inverse / leveraged vehicle — the suppression must be
/// phrase-shaped, not vocabulary-shaped: a word veto anywhere in the name
/// ("treasury", "bond") would also suppress genuine inverse bond funds
/// ("ProShares Short 20+ Year Treasury"), while an unconditional "ultra"
/// fragment misread "Ultra Short-Term Bond" duration funds as daily-reset
/// vehicles. Only a fragment inside a maturity phrase is a duration read.
/// Known cost (ruled 2026-08-05, piece-3 Codex round): "iShares Short Treasury
/// Bond"-style duration names flag as leveraged/inverse — still
/// `role_risk_only` either way, a wrong class label only; a big-run watch.
const SHORT_DURATION_PHRASES: &[&str] =
    &["short-term", "short term", "short duration", "short maturity"];

/// Name / mandate fragments that deterministically flag an **option-overlay** vehicle
/// (covered-call / buy-write / put-write / defined-outcome buffer funds). Unlike
/// leveraged / inverse, an overlay fund is **not** in the unpriceable list — it stays
/// on its class routing and carries the structural path-dependency flag instead
/// (`docs/portfolio-analysis.md` §Asset eligibility), which bars the Low risk tier
/// and rides the audit. The screen runs on fund names only, but still errs toward
/// false negatives like the movers screen it mirrors.
const OPTION_OVERLAY_FRAGMENTS: &[&str] = &[
    "covered call",
    "covered-call",
    "buywrite",
    "buy-write",
    "putwrite",
    "put-write",
    "premium income",
    "option income",
    "defined outcome",
    "buffer",
];

// ---- Inputs --------------------------------------------------------------------

/// Per-fund metadata from FMP `etf/info` + the sector / country weightings
/// (`docs/data-sources.md §Portfolio Analysis — endpoint surface`). Every field is
/// optional; a source that can't resolve a line records a gap.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FundData {
    pub symbol: String,
    /// Never blank: the adapter normalizes empty / whitespace-only strings to
    /// `None` (the quick check's comparability gates key on presence).
    pub name: Option<String>,
    /// The `etf/info` asset-class / mandate string (e.g. "Equity", "Fixed
    /// Income"). Never blank — same normalization contract as `name`.
    pub asset_class: Option<String>,
    /// Expense ratio as a decimal ratio (0.0009 for 9 bps).
    pub expense_ratio: Option<f64>,
    pub aum: Option<f64>,
    pub nav: Option<f64>,
    /// Sector weights as fractions (0–1), from `etf/sector-weightings`.
    pub sector_weights: Vec<(String, f64)>,
    /// Country weights as fractions (0–1), from `etf/country-weightings`.
    pub country_weights: Vec<(String, f64)>,
    /// The FMP `/profile` `isFund` flag — one leg of the closed-end detection
    /// (`None` = no profile resolved, which never guesses a CEF).
    pub profile_is_fund: Option<bool>,
    /// The FMP `/profile` description, kept verbatim for the closed-end fragment
    /// screen (`None` = no profile or no description served).
    pub profile_description: Option<String>,
    pub gaps: Vec<String>,
}

/// One per-sector aggregate P/E print (exchange-tagged), from `sector-pe-snapshot`
/// or a `historical-sector-pe` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectorPe {
    pub sector: String,
    pub exchange: String,
    pub date: String,
    pub pe: f64,
}

// ---- Strategy classification -----------------------------------------------------

/// The deterministic loop-time strategy class (`docs/portfolio-analysis.md` §Asset
/// eligibility): the asset class routes the computation because one generic fund
/// valuation cannot grade every vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FundStrategyClass {
    Equity,
    Bond,
    Commodity,
    LeveragedInverse,
    Unknown,
}

/// The classification result: the class, the structural flag, the US share where
/// readable, and — where the class is unpriceable — the typed role reason.
#[derive(Debug, Clone, PartialEq)]
pub struct FundClassification {
    pub class: FundStrategyClass,
    pub structural_flag: bool,
    pub us_share: Option<f64>,
    /// The card's classification label (e.g. "US equity fund", "bond fund").
    pub class_label: String,
    /// `None` when the exposure-priced path applies; `Some(reason)` when the class
    /// routes to `role_risk_only`.
    pub role_reason: Option<String>,
    /// The closed-end structure marker (the CEF leg) — orthogonal to the strategy
    /// class the way the overlay flag is: a bond CEF still routes bond. Detection
    /// is [`is_closed_end`]; the marker gates the price-vs-NAV read's rendering
    /// (`docs/portfolio-analysis.md` §Asset eligibility).
    pub is_cef: bool,
}

/// The closed-end fragments the detection screens the profile description for.
/// Substring matching covers the served variants — "closed-end", "closed-ended",
/// "closed end", "closed ended" (probe-verified 2026-08-21 on PDI / GAB / BST).
const CLOSED_END_FRAGMENTS: &[&str] = &["closed-end", "closed end"];

/// Deterministic closed-end detection: the profile's `isFund` flag AND a
/// closed-end fragment in its description — both legs required, so a missing or
/// ambiguous profile never guesses a CEF. `isFund` alone is FMP's flag for
/// mutual funds and CEFs alike, and the description fragment alone could ride a
/// manager's boilerplate; the conjunction is the drafted rule (probe-verified
/// 2026-08-21: PDI / GAB / BST carry both, `etf/info` serves them `[]`).
pub fn is_closed_end(fund: &FundData) -> bool {
    fund.profile_is_fund == Some(true)
        && fund
            .profile_description
            .as_deref()
            .map(|d| {
                let hay = d.to_ascii_lowercase();
                CLOSED_END_FRAGMENTS.iter().any(|f| hay.contains(f))
            })
            .unwrap_or(false)
}

/// The price-vs-NAV read: the **market price** against the reported NAV — never
/// the engine's NAV-fallback spot, which would fabricate an exact 0% premium
/// precisely when no market quote exists. Positive is a premium
/// (`docs/portfolio-analysis.md` §Asset eligibility — signal on the closed-end
/// form only; the caller gates rendering on [`is_closed_end`]). Both legs read
/// through the floor's usability test (`engine::usable_price`) and the read
/// itself must be finite — the raw quote reaches here beside the NAV-fallback
/// spot, so a non-finite print a producer other than the FMP parser handed the
/// fund must not re-enter as a premium the audit could not persist faithfully
/// (Codex I1, round 1).
pub fn nav_premium_read(current_price: Option<f64>, nav: Option<f64>) -> Option<f64> {
    match (engine::usable_price(current_price), engine::usable_price(nav)) {
        (Some(price), Some(nav)) => Some(price / nav - 1.0).filter(|p| p.is_finite()),
        _ => None,
    }
}

/// Classify a fund's strategy deterministically from its `etf/info` metadata and
/// weightings. Made at loop time — Step 3's eligibility used only Schwab instrument
/// identity (`docs/portfolio-workflow.md` §Step 3).
pub fn classify(fund: &FundData) -> FundClassification {
    let is_cef = is_closed_end(fund);
    // The marker rides the label so the card states the structure wherever the
    // class routes; the Unknown branch below replaces its label outright — for a
    // real CEF (empty `etf/info`) the structure is the one thing that IS known.
    let cef_suffix = |label: &str| -> String {
        if is_cef {
            format!("{label} (closed-end)")
        } else {
            label.to_string()
        }
    };
    let name_blob = format!(
        "{} {}",
        fund.name.as_deref().unwrap_or_default(),
        fund.asset_class.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    let leveraged_inverse = STRUCTURAL_FLAG_FRAGMENTS
        .iter()
        .any(|f| name_blob.contains(f))
        // The ambiguous fragments — bare "short" (SH-style inverse names carry
        // neither "-1x" nor "inverse") and "ultra" (UltraPro/UltraShort
        // leverage vs "Ultra Short-Term" duration) — count only outside a
        // duration phrase.
        || ((name_blob.contains("short") || name_blob.contains("ultra"))
            && !SHORT_DURATION_PHRASES.iter().any(|f| name_blob.contains(f)));
    if leveraged_inverse {
        return FundClassification {
            class: FundStrategyClass::LeveragedInverse,
            structural_flag: true,
            us_share: us_share(fund),
            class_label: cef_suffix("leveraged / inverse vehicle"),
            role_reason: Some(
                "structurally path-dependent (leveraged / inverse daily reset) — a \
                 buy-and-hold read is structurally unsound"
                    .to_string(),
            ),
            is_cef,
        };
    }

    // An option-overlay vehicle carries the structural path-dependency flag but keeps
    // its class routing — it is not in the unpriceable list, so a US equity
    // covered-call fund still prices, flagged.
    let overlay_flag = OPTION_OVERLAY_FRAGMENTS
        .iter()
        .any(|f| name_blob.contains(f));

    let class_str = fund.asset_class.as_deref().unwrap_or("").to_ascii_lowercase();
    let class = if class_str.contains("equity") || class_str.contains("stock") {
        FundStrategyClass::Equity
    } else if class_str.contains("fixed income") || class_str.contains("bond") {
        FundStrategyClass::Bond
    } else if class_str.contains("commodity") {
        FundStrategyClass::Commodity
    } else if !fund.sector_weights.is_empty() {
        // No usable class string, but sector weightings exist — the equity path's
        // fuel; adopted with the assumption recorded by the caller's gap manifest.
        FundStrategyClass::Equity
    } else {
        FundStrategyClass::Unknown
    };

    let us = us_share(fund);
    match class {
        FundStrategyClass::Bond => FundClassification {
            class,
            structural_flag: overlay_flag,
            us_share: us,
            class_label: cef_suffix("bond fund"),
            role_reason: Some(
                "bond fund — the on-plan surface carries no duration / credit / curve \
                 data to price it honestly (valuation recorded as a gap)"
                    .to_string(),
            ),
            is_cef,
        },
        FundStrategyClass::Commodity => FundClassification {
            class,
            structural_flag: overlay_flag,
            us_share: us,
            class_label: cef_suffix("commodity fund"),
            role_reason: Some(
                "commodity fund — no honest exposure-priced valuation on the on-plan \
                 surface (valuation recorded as a gap)"
                    .to_string(),
            ),
            is_cef,
        },
        // The one branch a real CEF reaches today: `etf/info` serves closed-end
        // funds an empty body (probe 2026-08-21), so no class string or weighting
        // exists — but the profile-detected structure is known, and the card
        // should say "closed-end fund", not "unresolved strategy class".
        FundStrategyClass::Unknown if is_cef => FundClassification {
            class,
            structural_flag: overlay_flag,
            us_share: us,
            class_label: "closed-end fund".to_string(),
            role_reason: Some(
                "closed-end fund — the current data surface serves no fund metadata \
                 for CEFs (`etf/info` is empty), so the exposure-priced valuation \
                 has no input"
                    .to_string(),
            ),
            is_cef,
        },
        FundStrategyClass::Unknown => FundClassification {
            class,
            structural_flag: overlay_flag,
            us_share: us,
            class_label: "fund with unresolved strategy class".to_string(),
            role_reason: Some(
                "strategy class unresolved and no usable sector weightings — the \
                 exposure-priced valuation has no input"
                    .to_string(),
            ),
            is_cef,
        },
        FundStrategyClass::Equity => {
            if fund.sector_weights.is_empty() {
                FundClassification {
                    class,
                    structural_flag: overlay_flag,
                    us_share: us,
                    class_label: cef_suffix("equity fund without usable weightings"),
                    role_reason: Some(
                        "no usable sector weighting set — the exposure-priced \
                         valuation has no input (the mutual-fund degrade)"
                            .to_string(),
                    ),
                    is_cef,
                }
            } else if us.map(|s| s < US_EXPOSURE_GUARD).unwrap_or(false) {
                FundClassification {
                    class,
                    structural_flag: overlay_flag,
                    us_share: us,
                    // The label describes the measurement, not a nationality claim:
                    // a 67%-US fund is not "ex-US", and attempt 2's model flagged
                    // exactly that (ruled 2026-08-13 — relabel, guard pinned;
                    // `docs/verification/2026-08-13-big-run-attempt-2.md`).
                    class_label: cef_suffix("equity fund below the US-exposure guard"),
                    role_reason: Some(format!(
                        "US exposure {:.0}% below the ≥ {:.0}% guard — an \
                         exchange-tagged US sector P/E is not an honest read on an \
                         international fund",
                        us.unwrap_or(0.0) * 100.0,
                        US_EXPOSURE_GUARD * 100.0
                    )),
                    is_cef,
                }
            } else {
                FundClassification {
                    class,
                    structural_flag: overlay_flag,
                    us_share: us,
                    class_label: cef_suffix("US equity fund"),
                    role_reason: None,
                    is_cef,
                }
            }
        }
        FundStrategyClass::LeveragedInverse => unreachable!("handled above"),
    }
}

/// The fund's US share from its country weightings, `None` when none are reported.
/// This is the ≥ 70% US-exposure guard's own read — every alias in
/// `US_LABELS` summed, capped at 1 — and the one the priced-fund prompt
/// renders, so the model never sees a share the guard did not (the 2026-08-24
/// review's Codex I8; `docs/portfolio-analysis.md` §Asset eligibility).
pub fn us_share(fund: &FundData) -> Option<f64> {
    if fund.country_weights.is_empty() {
        return None;
    }
    // Capped at 1: a share above 100% is only reachable through a
    // percent-served set misread as fractions, and this value feeds the
    // ≥70%-US pricing guard — the cap bounds that misread's blast radius.
    Some(
        fund.country_weights
            .iter()
            .filter(|(c, _)| US_LABELS.contains(&c.to_ascii_lowercase().trim()))
            .map(|(_, w)| w)
            .sum::<f64>()
            .min(1.0),
    )
}

// ---- The exposure-priced composite ------------------------------------------------

/// Blend the exchange-tagged per-sector P/Es into one per-sector read: the NYSE and
/// NASDAQ sector **earnings yields** averaged per sector (`docs/portfolio-analysis.md`
/// §Asset eligibility — the defined exchange blend), with a non-positive P/E excluded
/// as unusable rather than averaged in.
pub fn blend_sector_pes(rows: &[SectorPe]) -> HashMap<String, f64> {
    let mut yields: HashMap<String, Vec<f64>> = HashMap::new();
    for row in rows {
        if row.pe > 0.0 && row.pe.is_finite() {
            yields
                .entry(row.sector.to_ascii_lowercase())
                .or_default()
                .push(1.0 / row.pe);
        }
    }
    yields
        .into_iter()
        .map(|(sector, ys)| {
            let avg_yield = ys.iter().sum::<f64>() / ys.len() as f64;
            (sector, 1.0 / avg_yield)
        })
        .collect()
}

/// The covered-weight-renormalized composite earnings yield
/// (`docs/portfolio-analysis.md` §Asset eligibility): `Σ(wᵢ ÷ PEᵢ) ÷ Σwᵢ` across the
/// sectors with a usable P/E, so uncovered weight neither reads as zero earnings nor
/// lets a small priced slice extrapolate across the whole fund.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeYield {
    /// The composite earnings yield (decimal ratio).
    pub yield_value: f64,
    /// Covered share of total fund weight — the coverage-guard input.
    pub covered_share: f64,
}

pub fn composite_yield(
    weights: &[(String, f64)],
    blended_pe: &HashMap<String, f64>,
) -> Option<CompositeYield> {
    let mut covered = 0.0;
    let mut sum = 0.0;
    for (sector, w) in weights {
        // A non-finite weight is not a weight (a drifted string the adapter
        // parsed as NaN / inf): skipped, so it neither poisons `covered` nor
        // rides a NaN composite into the flat driver and the anchor sorts.
        if !w.is_finite() {
            continue;
        }
        if let Some(pe) = blended_pe.get(&sector.to_ascii_lowercase()) {
            covered += w;
            sum += w / pe;
        }
    }
    if covered <= 0.0 {
        return None;
    }
    let yield_value = sum / covered;
    // The finiteness / zero guard: a composite that is not a finite non-zero
    // yield prices no flat driver, and its reciprocal history sample would be
    // inf — read as absent rather than handed to the sorts.
    if !yield_value.is_finite() || yield_value == 0.0 {
        return None;
    }
    // The coverage-guard input is ABSOLUTE — the priced share of the whole
    // fund, not of whatever rows the feed happened to serve. Weights are
    // fund fractions by the adapter contract, so `covered` is that share
    // directly; renormalizing over the served rows' sum let a sparse response
    // (one 1.4% sector row) report 100% coverage and price the entire fund
    // off a sliver. The yield itself stays renormalized over covered weight —
    // uncovered weight neither reads as zero earnings nor extrapolates.
    Some(CompositeYield {
        yield_value,
        covered_share: covered.min(1.0),
    })
}

/// The constant-current-mix composite yield history (`docs/portfolio-analysis.md`
/// §Asset eligibility): today's weights over the historical sector multiples, sampled
/// at the trailing quarter ends, under the same blend / renormalization / coverage
/// convention as the snapshot — so the vs-own-history read compares like to like. A
/// sample date whose coverage falls below the guard is skipped rather than composed
/// off a sliver.
///
/// Each sample admits, per sector per exchange, only the latest print dated WITHIN
/// its own quarter — after the prior quarter end, on or before its own — so one
/// print backs at most one sample and the floor's count is a count of distinct
/// in-quarter observations (the 2026-08-24 review's Codex I2, ruled 2026-08-28: the
/// on-or-before select with no age bound let one stale print stand in for all
/// twelve samples, pass the eight-observation floor, and anchor twelve targets).
/// Dates compare as parsed calendar dates, never as strings; a print whose date
/// does not parse is inadmissible to every sample — the adapter drops such rows at
/// its shaper, and the parse here is the belt behind that brace.
pub fn composite_yield_history(
    weights: &[(String, f64)],
    history: &HashMap<String, Vec<SectorPe>>,
    as_of: NaiveDate,
) -> Vec<DatedValue> {
    // Parse once: each sector's datable prints, an undatable row dropped.
    let dated: Vec<(&String, Vec<(NaiveDate, &SectorPe)>)> = history
        .iter()
        .map(|(sector, prints)| {
            let parsed = prints
                .iter()
                .filter_map(|p| {
                    NaiveDate::parse_from_str(&p.date, "%Y-%m-%d")
                        .ok()
                        .map(|d| (d, p))
                })
                .collect();
            (sector, parsed)
        })
        .collect();
    let mut out = Vec::new();
    for q in 1..=HISTORY_SAMPLE_QUARTERS {
        let sample_date = quarter_end_before(as_of, q);
        let prior_quarter_end = quarter_end_before(as_of, q + 1);
        let date_str = sample_date.format("%Y-%m-%d").to_string();
        // Per sector: the latest print within the sample's own quarter, per
        // exchange, then the same blend as the snapshot.
        let mut rows: Vec<SectorPe> = Vec::new();
        for (sector, prints) in &dated {
            let mut latest_by_exchange: HashMap<&str, (NaiveDate, &SectorPe)> = HashMap::new();
            for (d, p) in prints {
                if *d <= prior_quarter_end || *d > sample_date {
                    continue;
                }
                let slot = latest_by_exchange
                    .entry(p.exchange.as_str())
                    .or_insert((*d, p));
                if *d > slot.0 {
                    *slot = (*d, p);
                }
            }
            for (_, p) in latest_by_exchange.values() {
                rows.push(SectorPe {
                    sector: (*sector).clone(),
                    exchange: p.exchange.clone(),
                    date: p.date.clone(),
                    pe: p.pe,
                });
            }
        }
        let blended = blend_sector_pes(&rows);
        if let Some(c) = composite_yield(weights, &blended) {
            if c.covered_share >= PE_COVERAGE_GUARD {
                out.push(DatedValue {
                    date: date_str,
                    value: c.yield_value,
                });
            }
        }
    }
    out.sort_by(|a, b| a.date.cmp(&b.date));
    out
}

/// The `q`-th calendar quarter end strictly before `as_of` (q = 1 is the most recent).
fn quarter_end_before(as_of: NaiveDate, q: usize) -> NaiveDate {
    use chrono::Datelike;
    let mut year = as_of.year();
    // The most recent completed quarter end ≤ as_of.
    let mut month_end = match as_of.month() {
        1..=3 => {
            year -= 1;
            12
        }
        4..=6 => 3,
        7..=9 => 6,
        _ => 9,
    };
    for _ in 1..q {
        if month_end == 3 {
            month_end = 12;
            year -= 1;
        } else {
            month_end -= 3;
        }
    }
    let day = match month_end {
        3 => 31,
        6 => 30,
        9 => 30,
        _ => 31,
    };
    NaiveDate::from_ymd_opt(year, month_end, day).expect("valid quarter end")
}

/// The fund half of a holding's dossier: the per-fund metadata plus the run-level
/// sector-P/E surface the fund engine reads. Assembled by the job (which memoizes
/// the sector-P/E snapshot and per-sector histories across funds —
/// `docs/portfolio-workflow.md` §Step 6a) so the engine stays pure.
#[derive(Debug, Clone, PartialEq)]
pub struct FundContext {
    pub fund: FundData,
    pub sector_pe: Vec<SectorPe>,
    /// Keyed by lowercase sector label.
    pub sector_pe_history: HashMap<String, Vec<SectorPe>>,
    pub as_of: NaiveDate,
    /// The CFTC underlying-positioning read for a commodity / macro fund — the
    /// run-level COT pull mapped onto this fund's underlying by
    /// [`cot_contract_for_fund`] (`docs/data-sources.md §CFTC`; positioning is
    /// layer-(c) evidence, held out of every sub-score). `None` = no mapping,
    /// or the mapped contract's row didn't land this run (fail-soft).
    pub positioning: Option<crate::data_sources::CotPositioning>,
}

/// Map a commodity / macro fund onto one of the run-level COT bellwether
/// contracts (by CFTC contract code) for the underlying-positioning read — a
/// deterministic keyword match over the fund's `etf/info` name + asset-class
/// strings (drafted, calibratable). A fund whose underlying isn't among the
/// tracked contracts returns `None` — the doc-sanctioned fail-soft
/// (`docs/data-sources.md §CFTC`: "A fund whose underlying isn't among these
/// contracts fail-softs to no positioning read").
pub fn cot_contract_for_fund(fund: &FundData) -> Option<&'static str> {
    let hay = format!(
        "{} {}",
        fund.name.as_deref().unwrap_or(""),
        fund.asset_class.as_deref().unwrap_or("")
    )
    .to_ascii_lowercase();
    if hay.trim().is_empty() {
        return None;
    }
    // Commodity underlyings (disaggregated managed-money contracts).
    if hay.contains("gold") {
        return Some("088691");
    }
    if hay.contains("crude") || hay.contains("oil") {
        return Some("067651");
    }
    if hay.contains("copper") {
        return Some("085692");
    }
    // Macro underlyings (Traders in Financial Futures contracts).
    if hay.contains("s&p 500") || hay.contains("s&p500") || hay.contains("500 index") {
        return Some("13874A");
    }
    if hay.contains("nasdaq") {
        return Some("209742");
    }
    if hay.contains("dollar") {
        return Some("098662");
    }
    if hay.contains("treasury") {
        // Two real name shapes collide on the word "short": a duration name
        // ("iShares Short Treasury Bond", "Short-Term US Treasury") and an
        // inverse fund's direction ("ProShares Short 20+ Year Treasury"). A
        // long-maturity phrase settles it — it overrides a directional
        // "short", since an inverse long-duration fund's underlying is still
        // the long end; otherwise any short / 1-3 signal reads short-duration.
        // The 10-Year note is the bellwether default.
        let long_maturity = ["20+", "10-year", "10 year", "7-10", "long-term", "long term", "long duration", "extended duration"]
            .iter()
            .any(|k| hay.contains(k));
        let short_signal =
            hay.contains("short") || hay.contains("1-3") || hay.contains("0-3");
        return if short_signal && !long_maturity {
            Some("042601")
        } else {
            Some("043602")
        };
    }
    None
}

// ---- The fund engine ---------------------------------------------------------------

/// The engine-computed half of a `role_risk_only` readout — the model authors only
/// the role prose on top of this.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleRiskReadout {
    pub class_label: String,
    /// Top exposure weights (sector where present, else country).
    pub exposure_tilt: Vec<(String, f64)>,
    pub expense_ratio: Option<f64>,
    /// Annualized realized volatility, where computable.
    pub observable_risk: Option<f64>,
    pub structural_flag: bool,
    /// The closed-end structure marker ([`is_closed_end`]) — gates the
    /// price-vs-NAV rendering to the closed-end form.
    pub is_cef: bool,
    /// Price vs NAV ([`nav_premium_read`]), populated on the closed-end form
    /// only — `None` on a CEF is the named gap (a missing NAV, or a missing
    /// market quote against a present NAV — the gap text names which), pushed
    /// into `evidence_gaps` rather than rendered as a number.
    pub nav_premium: Option<f64>,
    pub evidence_gaps: Vec<String>,
}

/// What the fund engine resolved to: the priced branch (the shared [`EngineOutput`]),
/// the typed role / risk readout, or the evidence floor's fund-analog abstention.
#[derive(Debug, Clone, PartialEq)]
pub enum FundEngineVerdict {
    Priced(Box<EngineOutput>),
    RoleRiskOnly(Box<RoleRiskReadout>),
    InsufficientEvidence(String),
}

/// Everything the fund engine reads, assembled by the job (the engine stays pure).
pub struct FundEngineInputs<'a> {
    pub fund: &'a FundData,
    /// The fund's quote / price surface (quote, dated closes, TTM distributions ride
    /// the same per-symbol financials pull a stock uses).
    pub financials: &'a CompanyFinancials,
    /// Today's `sector-pe-snapshot` rows (both exchanges).
    pub sector_pe: &'a [SectorPe],
    /// Per-sector `historical-sector-pe` rows (both exchanges), keyed by lowercase
    /// sector label.
    pub sector_pe_history: &'a HashMap<String, Vec<SectorPe>>,
    pub rates: &'a RateAnchors,
    pub as_of: NaiveDate,
}

/// Analyze a fund holding down the reduced path (`docs/portfolio-analysis.md` §Asset
/// eligibility): classify, then either the exposure-priced equity-fund computation —
/// real valuation / risk, neutral-imputed quality, the fund-form v2 targets, the fund
/// tier, the hurdle — or the typed `role_risk_only` readout; genuinely missing
/// floor-bearing data abstains instead.
pub fn analyze_fund(inp: &FundEngineInputs) -> FundEngineVerdict {
    let fund = inp.fund;
    let fin = inp.financials;

    // The evidence floor's fund analog, floor-bearing legs first: a usable current
    // quote / NAV — finite and strictly positive (`engine::usable_price`), never
    // mere presence, so a served zero or negative quote is no price and falls to
    // a usable NAV rather than masking it (Codex I1, ruled 2026-08-28) — and the
    // `etf/info` surface.
    let Some(spot) =
        engine::usable_price(fin.current_price).or(engine::usable_price(fund.nav))
    else {
        return FundEngineVerdict::InsufficientEvidence(
            "no usable current quote or NAV for the fund — nothing to value against"
                .to_string(),
        );
    };
    // Classification runs before the metadata floor: a detected closed-end fund
    // with an empty `etf/info` (the surface serves CEFs `[]` — probe 2026-08-21)
    // is a *structural* thin-surface class, not deficient evidence, so it takes
    // the typed role / risk readout below instead of abstaining every run.
    let classification = classify(fund);
    let info_present = fund.asset_class.is_some()
        || fund.expense_ratio.is_some()
        || fund.name.is_some()
        || !fund.sector_weights.is_empty();
    if !info_present && !classification.is_cef {
        return FundEngineVerdict::InsufficientEvidence(
            "fund metadata (etf/info) unavailable — the fund analog's floor-bearing \
             input is missing"
                .to_string(),
        );
    }

    let vol = per_period_volatility(fin);
    let annual_vol = vol.map(|v| v * 15.87);
    // Finite-or-absent like every engine metric (Codex I16): the risk leg
    // scales it into a required sub-score.
    let drawdown = engine::finite(engine::max_drawdown(&fin.daily_closes, &fin.price_history));

    // A structurally unpriceable class takes the typed role / risk readout — never
    // `insufficient-evidence` (the evidence isn't deficient; the class is).
    if let Some(reason) = &classification.role_reason {
        let tilt = if !fund.sector_weights.is_empty() {
            top_weights(&fund.sector_weights)
        } else {
            top_weights(&fund.country_weights)
        };
        // The closed-end read (`docs/portfolio-analysis.md` §Asset eligibility):
        // computed from the market price only, absent reads as the named gap —
        // and populated only on the closed-end form, so a non-CEF fund's
        // transient premium never rides the verdict, prompts, or audit metrics.
        let nav_premium = if classification.is_cef {
            nav_premium_read(fin.current_price, fund.nav)
        } else {
            None
        };
        let mut gaps = fund.gaps.clone();
        if classification.is_cef {
            if !info_present {
                gaps.push(
                    "fund metadata (etf/info) is empty for closed-end funds on the \
                     current data surface — expense ratio and exposure unavailable"
                        .to_string(),
                );
            }
            if nav_premium.is_none() {
                // The gap names what is actually absent, keyed on USABILITY
                // on both legs, not presence: the spot floor accepts NAV as
                // the price fallback, and a producer other than the FMP parser
                // (which drops a zero or negative print at the parse — Codex
                // I1) can still hand this branch one, so a presence test would
                // misstate both the quote-missing and the quote-unusable cases
                // (Codex 2026-08-21 rounds 3–4, finding 2; the NAV leg and the
                // non-finite read off I1's round 2).
                let cause = match (
                    engine::usable_price(fund.nav),
                    engine::usable_price(fin.current_price),
                ) {
                    (None, _) => "no usable NAV for closed-end funds on the current data surface",
                    (Some(_), None) => "no usable market quote to read against the reported NAV",
                    // Both legs usable, yet no read: the quotient did not come
                    // out finite — neither leg is the thing missing.
                    (Some(_), Some(_)) => "the price-vs-NAV read did not come out finite",
                };
                gaps.push(format!("price-vs-NAV unavailable — {cause}"));
            }
        }
        gaps.push(reason.clone());
        return FundEngineVerdict::RoleRiskOnly(Box::new(RoleRiskReadout {
            class_label: classification.class_label,
            exposure_tilt: tilt,
            expense_ratio: fund.expense_ratio,
            observable_risk: annual_vol,
            structural_flag: classification.structural_flag,
            is_cef: classification.is_cef,
            nav_premium,
            evidence_gaps: gaps,
        }));
    }

    // The remaining floor-bearing fund-analog legs, enforced on the exposure-priced
    // branch (`docs/portfolio-analysis.md` §Evidence floor: quote / NAV, `etf/info`,
    // the expense ratio, and enough weighting coverage to read exposure). The
    // structural routing above takes precedence — a `role_risk_only` class is never
    // an abstention, so these checks sit after it.
    if fund.expense_ratio.is_none() {
        return FundEngineVerdict::InsufficientEvidence(
            "expense ratio missing — a floor-bearing fund-analog input (etf/info)"
                .to_string(),
        );
    }
    if classification.us_share.is_none() {
        return FundEngineVerdict::InsufficientEvidence(
            "no country weightings — the ≥ 70% US-exposure guard cannot be verified, \
             a floor-bearing weighting-coverage input on the exposure-priced branch"
                .to_string(),
        );
    }

    // The priced equity-fund path: the exposure-priced valuation under its coverage
    // guard, read against its own constant-current-mix history.
    let blended_now = blend_sector_pes(inp.sector_pe);
    let Some(composite) = composite_yield(&fund.sector_weights, &blended_now) else {
        return FundEngineVerdict::InsufficientEvidence(
            "no P/E-usable sector overlap between the fund's weightings and the \
             sector-P/E snapshot"
                .to_string(),
        );
    };
    if composite.covered_share < PE_COVERAGE_GUARD {
        return FundEngineVerdict::InsufficientEvidence(format!(
            "fund valuation coverage {:.0}% below the ≥ {:.0}% P/E-usable guard — \
             valuation recorded as a gap rather than lettered off a sliver",
            composite.covered_share * 100.0,
            PE_COVERAGE_GUARD * 100.0
        ));
    }
    let history = composite_yield_history(&fund.sector_weights, inp.sector_pe_history, inp.as_of);
    if history.len() < MIN_COMPOSITE_HISTORY {
        return FundEngineVerdict::InsufficientEvidence(format!(
            "only {} constant-mix composite history samples on distinct in-quarter prints \
             (need {MIN_COMPOSITE_HISTORY}) — the vs-own-history valuation read has no basis",
            history.len()
        ));
    }

    // Valuation: what the mix costs now versus what it has cost — the percentile rank
    // of the current composite yield in its own history (a higher yield is cheaper).
    // `<=` counts exact ties as cheap, so a degenerate flat history scores 100
    // rather than neutral — accepted as cosmetic (ruled 2026-08-05, piece-3 walk).
    let below = history
        .iter()
        .filter(|h| h.value <= composite.yield_value)
        .count();
    let valuation = (below as f64 / history.len() as f64) * 100.0;

    // Risk: realized volatility plus drawdown (higher = safer, like the stock leg).
    let risk = {
        let vol_leg = vol.map(|v| engine::scale(v, 0.04, 0.0));
        let dd_leg = drawdown.map(|d| engine::scale(d, 0.6, 0.0));
        match (vol_leg, dd_leg) {
            (None, None) => None,
            (a, b) => Some((a.unwrap_or(50.0) + b.unwrap_or(50.0)) / 2.0),
        }
    };
    let Some(risk) = risk else {
        return FundEngineVerdict::InsufficientEvidence(
            "no price history for the fund's risk read — the second real fund \
             sub-score has no input"
                .to_string(),
        );
    };

    // Momentum rides as context (outside the letter) and IS the stock read —
    // `engine::momentum_score` over `base_metrics`' 180-day `price_history` leg,
    // one window and one band. Scoring the ~1,600-day `daily_closes` here pinned
    // nearly every fund at 0 / 100: a multi-year cumulative return against a band
    // tuned to 180 days (the 2026-08-24 review's fund-momentum finding;
    // introduced at `grade-v2.2`, carried by current `grade-v2.3`).
    let mut metrics = base_metrics(fin);
    let momentum = engine::momentum_score(&metrics);

    // The letter: real valuation / risk + the neutral-imputed absent quality axis —
    // the priced-fund grade contract, with the visible low-confidence marker.
    let sub_scores = SubScores {
        quality: 50.0,
        valuation,
        momentum: momentum.unwrap_or(50.0),
        risk,
    };
    let grade = engine::grade_from_subscores(&sub_scores);

    // The fund-form v2 targets: driver = spot × composite yield (flat), the anchor
    // spreads from the constant-mix history against the dated DGS10 join, TTM
    // distributions in the total return.
    let implied_eps = spot * composite.yield_value;
    // The dated-rate join is per-leg (the stock builder's rule): a sample with no
    // DGS10 on or before its date loses only its spread — the raw multiple stays
    // admissible, so a failed history request degrades to the raw-percentile
    // fallback, never straight to the current-multiple carry.
    let observations: Vec<AnchorObservation> = history
        .iter()
        .map(|h| AnchorObservation {
            spread: engine::latest_on_or_before(&inp.rates.dgs10_history, &h.date)
                .map(|dgs10_t| h.value - dgs10_t),
            raw_multiple: 1.0 / h.value,
        })
        .collect();
    // Finite by the FMP shaper's overflow rejection; held here for any other
    // producer, since the basis persists it as a required float (Codex I16).
    let distributions = engine::finite(fin.ttm_dividends_per_share).unwrap_or(0.0);
    let floor = engine::dispersion_floor(vol);
    let scenario = engine::spread_anchored_scenarios(
        spot,
        [implied_eps, implied_eps, implied_eps],
        &observations,
        inp.rates.dgs10,
        distributions,
        // The fund driver is deliberately flat, so on the carry path both scenario
        // axes collapse — the shared floor keeps the three-state hurdle honest there
        // too (`docs/portfolio-analysis.md` §Starting parameters).
        floor,
    );

    metrics.expense_ratio = fund.expense_ratio;
    // Market price only — the NAV-fallback `spot` would fabricate an exact 0%
    // premium precisely when no quote exists ([`nav_premium_read`]).
    metrics.nav_premium = nav_premium_read(fin.current_price, fund.nav);
    metrics.composite_coverage = Some(composite.covered_share);

    // The uncovered slice is reported beside the read, never averaged in
    // (`docs/portfolio-analysis.md` §Asset eligibility), and an option-overlay flag
    // is recorded where it was detected — carried, never silently absorbed.
    let mut engine_notes: Vec<String> = Vec::new();
    if composite.covered_share < 1.0 {
        engine_notes.push(format!(
            "composite P/E coverage {:.0}% of fund weight — the uncovered {:.0}% is \
             reported beside the valuation read, never averaged in",
            composite.covered_share * 100.0,
            (1.0 - composite.covered_share) * 100.0
        ));
    }
    if classification.structural_flag {
        engine_notes.push(
            "option-overlay structural path-dependency flag (name / mandate screen) — \
             the overlay reshapes the return path the exposure composite prices, so \
             the flag rides the audit and bars the Low risk tier"
                .to_string(),
        );
    }

    let targets = engine::build_price_targets(
        spot,
        &scenario,
        &metrics,
        "fund exposure composite",
        true,
        // The v4 anchor bound and trough release are stock-form provenance —
        // the composite path has neither.
        0,
        false,
    );
    // The engine's output gate (`engine::price_targets_finite`): a non-finite
    // composite prices every scenario non-finite — the fund exits as
    // insufficient evidence, never a `null` target the store cannot read back.
    // (A non-finite quote never reaches here: the usability floor above stops
    // it.)
    if !engine::price_targets_finite(&targets) {
        return FundEngineVerdict::InsufficientEvidence(
            "non-finite scenario pricing: a feed extreme overflowed the composite driver \
             — no finite target to persist"
                .to_string(),
        );
    }
    let tier = engine::assign_fund_tier(
        // Leveraged / inverse never reaches the priced path (it routes to
        // `role_risk_only` above); the comparison keeps the High leg honest anyway.
        classification.class == FundStrategyClass::LeveragedInverse,
        classification.structural_flag,
        annual_vol,
        drawdown,
    );
    let hurdle = engine::hurdle_read(&scenario, inp.rates.dgs2, tier);
    let meta = TargetMeta {
        driver_rung: "fund exposure composite".to_string(),
        rate_anchored: scenario.rate_anchored,
        anchor_observations: scenario.anchor_observations,
        flat_driver: true,
        degenerate_scenarios: scenario.degenerate_scenarios,
        monotonicity_repaired: scenario.monotonicity_repaired,
        current_multiple_carry: scenario.current_multiple_carry,
        consensus_rows: None,
        consensus_near_weight: None,
        clamp_flattened: false,
        dispersion_floor_applied: scenario.dispersion_floor_applied,
        anchor_bounded: 0,
        clamp_released: false,
        parameter_version: engine::SCENARIO_TARGET_PARAMETER_VERSION.to_string(),
    };

    FundEngineVerdict::Priced(Box::new(EngineOutput {
        sub_scores,
        grade,
        metrics,
        price_targets: targets,
        risk_tier: tier,
        tier_gaps: engine_notes,
        hurdle,
        target_meta: meta,
        // The quality axis is structurally absent and neutral-imputed — the letter
        // always carries the visible low-confidence marker on this branch.
        low_confidence_grade: true,
        // The deterministic classification is shown on the card — the priced branch
        // included, an option-overlay flag riding beside it.
        fund_class_label: Some(classification.class_label),
        structural_flag: classification.structural_flag,
        quick_basis: Some(engine::QuickCheckBasis {
            spot,
            drivers: [implied_eps, implied_eps, implied_eps],
            spread_percentiles: scenario.spread_percentiles,
            raw_percentiles: scenario.raw_percentiles,
            forward_dividends: distributions,
            dispersion_floor: floor,
            consensus_eps_mid: None,
        }),
        // The fund form prices a deliberately flat synthetic driver over the
        // exposure composite (the settled design, ruled 2026-08-21) — no driver
        // trajectory exists to invert.
        implied_expectations: None,
    }))
}

/// The fund exposure comparators the quick check's fund evidence-event legs read
/// (`docs/portfolio-analysis.md` §Starting parameters — a material `etf/info`
/// change; the US share crossing the ≥ 70% guard; a top-sector shift), computed at
/// full-pass time on **either** verdict branch and persisted on the holding's audit
/// so the engine-only sweep has a stored side to compare a fresh print against.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FundExposureBasis {
    /// The deterministic strategy-classification label the pass computed.
    pub class_label: String,
    pub expense_ratio: Option<f64>,
    /// The US country-weight share the ≥ 70% guard read.
    pub us_share: Option<f64>,
    /// The largest sector weight `(label, weight)`.
    pub top_sector: Option<(String, f64)>,
    /// The structural path-dependency flag the classification carried (an
    /// option-overlay vehicle keeps its class routing but is flagged) — persisted
    /// so the sweep can see a flag transition that changes no label
    /// (`docs/portfolio-analysis.md` §Starting parameters, the every-fund
    /// asset-class-change leg: a structural-flag reclassification counts).
    pub structural_flag: bool,
}

/// Build the [`FundExposureBasis`] from a fund's fresh metadata — shared by the
/// full pass (persisting the stored side) and the quick check (computing the fresh
/// side), so the two compare like with like.
pub fn exposure_basis(fund: &FundData) -> FundExposureBasis {
    let classification = classify(fund);
    // A non-finite weight is not a weight (the adapter drops such rows; this
    // holds for any other producer) — it persists on the basis as a required
    // float, so it never enters the comparison (Codex I7 / I16).
    let top_sector = fund
        .sector_weights
        .iter()
        .filter(|(_, w)| w.is_finite())
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .cloned();
    FundExposureBasis {
        class_label: classification.class_label,
        expense_ratio: fund.expense_ratio,
        us_share: classification.us_share,
        top_sector,
        structural_flag: classification.structural_flag,
    }
}

/// The top exposure weights for the readout's tilt line, largest first, capped at 5.
fn top_weights(weights: &[(String, f64)]) -> Vec<(String, f64)> {
    // Same rule as `exposure_basis`: a non-finite weight never reaches the
    // readout's persisted tilt (Codex I7 / I16).
    let mut sorted: Vec<(String, f64)> =
        weights.iter().filter(|(_, w)| w.is_finite()).cloned().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted.truncate(5);
    sorted
}

/// Per-period volatility over whichever price history is present (dated preferred).
fn per_period_volatility(fin: &CompanyFinancials) -> Option<f64> {
    let closes: Vec<f64> = if !fin.daily_closes.is_empty() {
        fin.daily_closes.iter().map(|d| d.value).collect()
    } else {
        fin.price_history.clone()
    };
    engine::return_volatility(&closes)
}

/// The fund's base metrics (price-derived legs only; the statement legs stay `None`).
///
/// The two price legs come from [`engine::compute_metrics`] — the 180-day
/// `price_history` — **not** the ~1,600-day dated `daily_closes` the volatility
/// helper above prefers. `TrailingReturn` and `ReturnVolatility` are both
/// fund-computable ledger series, and the quick check evaluates them off
/// `price_history` (`quick_check.rs`, the sweep's own EOD pull). Authoring them
/// here on the deep history would author a condition on one window and evaluate it
/// on another, so a fund's falsifier could confirm a breach with no change in the
/// thesis. This mirrors the role-risk branch, which was pointed at
/// `compute_metrics` for exactly this reason (`pipeline.rs`).
///
/// The momentum sub-score reads the same `trailing_return` leg through
/// `engine::momentum_score` (`analyze_fund`). The deep history backs the volatility
/// and drawdown reads — the risk sub-score's two legs, the dispersion floor, and the
/// tier — never momentum; each is authored once per run and never re-evaluated by a
/// sweep.
fn base_metrics(fin: &CompanyFinancials) -> ComputedMetrics {
    let price_legs = engine::compute_metrics(fin);
    ComputedMetrics {
        return_volatility: price_legs.return_volatility,
        trailing_return: price_legs.trailing_return,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weights() -> Vec<(String, f64)> {
        vec![
            ("Technology".to_string(), 0.50),
            ("Financial Services".to_string(), 0.30),
            ("Energy".to_string(), 0.20),
        ]
    }

    fn snapshot() -> Vec<SectorPe> {
        let mut rows = Vec::new();
        for (sector, nyse, nasdaq) in [
            ("Technology", 30.0, 34.0),
            ("Financial Services", 14.0, 16.0),
            ("Energy", 11.0, 13.0),
        ] {
            rows.push(SectorPe {
                sector: sector.to_string(),
                exchange: "NYSE".to_string(),
                date: "2026-07-15".to_string(),
                pe: nyse,
            });
            rows.push(SectorPe {
                sector: sector.to_string(),
                exchange: "NASDAQ".to_string(),
                date: "2026-07-15".to_string(),
                pe: nasdaq,
            });
        }
        rows
    }

    fn history() -> HashMap<String, Vec<SectorPe>> {
        // Quarterly prints back through 2022 for each sector, both exchanges —
        // one mid-quarter-end-month print per quarter, so every sampled quarter
        // finds a print within it.
        let mut map: HashMap<String, Vec<SectorPe>> = HashMap::new();
        let dates = [
            "2022-09-15", "2022-12-15", "2023-03-15", "2023-06-15", "2023-09-15",
            "2023-12-15", "2024-03-15", "2024-06-15", "2024-09-15", "2024-12-15",
            "2025-03-15", "2025-06-15", "2025-09-15", "2025-12-15", "2026-03-15",
            "2026-06-15",
        ];
        for (sector, base_pe) in [
            ("Technology", 26.0),
            ("Financial Services", 13.0),
            ("Energy", 10.0),
        ] {
            let mut prints = Vec::new();
            for (i, date) in dates.iter().enumerate() {
                for exchange in ["NYSE", "NASDAQ"] {
                    prints.push(SectorPe {
                        sector: sector.to_string(),
                        exchange: exchange.to_string(),
                        date: date.to_string(),
                        pe: base_pe + 0.2 * i as f64,
                    });
                }
            }
            map.insert(sector.to_ascii_lowercase(), prints);
        }
        map
    }

    fn fund() -> FundData {
        FundData {
            symbol: "VTI".to_string(),
            name: Some("Total US Market ETF".to_string()),
            asset_class: Some("Equity".to_string()),
            expense_ratio: Some(0.0003),
            aum: Some(4.0e11),
            nav: Some(280.0),
            sector_weights: weights(),
            country_weights: vec![("United States".to_string(), 0.99)],
            profile_is_fund: None,
            profile_description: None,
            gaps: vec![],
        }
    }

    fn financials(price: f64) -> CompanyFinancials {
        CompanyFinancials {
            symbol: "VTI".to_string(),
            current_price: Some(price),
            price_history: vec![250.0, 260.0, 270.0, 282.0],
            daily_closes: vec![
                DatedValue { date: "2026-04-01".into(), value: 250.0 },
                DatedValue { date: "2026-05-01".into(), value: 260.0 },
                DatedValue { date: "2026-06-01".into(), value: 270.0 },
                DatedValue { date: "2026-07-15".into(), value: price },
            ],
            ttm_dividends_per_share: Some(3.6),
            ..Default::default()
        }
    }

    fn rates() -> RateAnchors {
        let dates = [
            "2022-09-01", "2023-01-01", "2023-06-01", "2024-01-01", "2024-06-01",
            "2025-01-01", "2025-06-01", "2026-01-01", "2026-06-01",
        ];
        RateAnchors {
            dgs2: 0.04,
            dgs10: 0.045,
            dgs10_history: dates
                .iter()
                .map(|d| DatedValue { date: d.to_string(), value: 0.04 })
                .collect(),
            history_gap: None,
            ..Default::default()
        }
    }

    fn as_of() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 16).unwrap()
    }

    #[test]
    fn cot_mapping_matches_bellwether_underlyings_and_fail_softs_the_rest() {
        let fund = |name: &str, class: &str| FundData {
            symbol: "X".into(),
            name: (!name.is_empty()).then(|| name.to_string()),
            asset_class: (!class.is_empty()).then(|| class.to_string()),
            ..Default::default()
        };
        let code = |name: &str, class: &str| cot_contract_for_fund(&fund(name, class));
        // Commodity underlyings → the disaggregated contracts.
        assert_eq!(code("SPDR Gold Shares", "Commodity"), Some("088691"));
        assert_eq!(code("United States Oil Fund", "Commodity"), Some("067651"));
        assert_eq!(code("US Copper Index Fund", "Commodity"), Some("085692"));
        // Macro underlyings → the TFF contracts.
        assert_eq!(code("SPDR S&P 500 ETF Trust", "Equity"), Some("13874A"));
        assert_eq!(code("Invesco Nasdaq-100 ETF", "Equity"), Some("209742"));
        assert_eq!(code("Invesco DB US Dollar Index Bullish", "Currency"), Some("098662"));
        assert_eq!(code("iShares 20+ Year Treasury Bond", "Fixed Income"), Some("043602"));
        assert_eq!(code("Schwab Short-Term US Treasury", "Fixed Income"), Some("042601"));
        assert_eq!(code("SPDR 1-3 Month Treasury Bill? 1-3", "Fixed Income"), Some("042601"));
        // An inverse long-duration fund: "Short" is direction, not maturity —
        // the long-maturity phrase overrides it and the underlying stays the
        // long-end note (Codex 2026-08-20, finding 3).
        assert_eq!(
            code("ProShares Short 20+ Year Treasury", "Fixed Income"),
            Some("043602")
        );
        // A bare-"short" duration name with no long-maturity phrase reads
        // short-duration (Codex round 2: the SHV-style shape).
        assert_eq!(
            code("iShares Short Treasury Bond", "Fixed Income"),
            Some("042601")
        );
        // No tracked underlying — or no identity at all — fail-softs to no read.
        assert_eq!(code("Vanguard Total Stock Market", "Equity"), None);
        assert_eq!(code("", ""), None);
    }

    /// A fund's `TrailingReturn` / `ReturnVolatility` ledger conditions are authored
    /// on the full pass and evaluated by the quick check, which reads the 180-day
    /// `price_history`. Authoring them on the ~1,600-day `daily_closes` put the two
    /// on different windows, so a falsifier could confirm a breach with the thesis
    /// intact. The full pass's ledger surface must match the sweep's.
    #[test]
    fn base_metrics_price_legs_match_the_window_the_quick_check_evaluates() {
        // The two histories disagree sharply: the deep series has tripled, the
        // 180-day window is flat. The ledger must see the flat one.
        let fin = CompanyFinancials {
            symbol: "VTI".to_string(),
            current_price: Some(300.0),
            price_history: vec![297.0, 298.0, 299.0, 300.0],
            daily_closes: vec![
                DatedValue { date: "2022-01-03".into(), value: 100.0 },
                DatedValue { date: "2024-01-02".into(), value: 200.0 },
                DatedValue { date: "2026-07-15".into(), value: 300.0 },
            ],
            ..Default::default()
        };

        let m = base_metrics(&fin);
        let sweep = engine::compute_metrics(&fin);
        assert_eq!(
            m.trailing_return, sweep.trailing_return,
            "the full pass must author on the window the sweep evaluates"
        );
        assert_eq!(m.return_volatility, sweep.return_volatility);

        // Concretely: ~1% off the 180-day window, not the ~200% the deep series
        // would have authored.
        let tr = m.trailing_return.expect("both closes present");
        assert!(tr < 0.05, "trailing return {tr} came from the deep history");

        // The deep history still backs the volatility and drawdown reads (risk
        // legs, dispersion floor, tier) — never momentum — authored once per run,
        // never re-evaluated, so no second window disagrees.
        assert!(
            per_period_volatility(&fin) != m.return_volatility,
            "fixture sanity: the two windows genuinely differ"
        );
    }

    /// The fund's momentum sub-score is the stock read — `engine::momentum_score`
    /// over the 180-day `price_history` leg. Scoring the ~1,600-day `daily_closes`
    /// pinned nearly every fund at 0 / 100: a multi-year cumulative return against
    /// a band tuned to 180 days (the 2026-08-24 review's fund-momentum finding).
    #[test]
    fn fund_momentum_is_the_stock_read_over_the_short_window() {
        // The deep series has tripled; the 180-day window is up ~1%.
        let fin = CompanyFinancials {
            symbol: "VTI".to_string(),
            current_price: Some(300.0),
            price_history: vec![297.0, 298.0, 299.0, 300.0],
            daily_closes: vec![
                DatedValue {
                    date: "2022-01-03".into(),
                    value: 100.0,
                },
                DatedValue {
                    date: "2024-01-02".into(),
                    value: 200.0,
                },
                DatedValue {
                    date: "2026-07-15".into(),
                    value: 300.0,
                },
            ],
            ttm_dividends_per_share: Some(3.6),
            ..Default::default()
        };
        let inputs = FundEngineInputs {
            fund: &fund(),
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        let FundEngineVerdict::Priced(out) = analyze_fund(&inputs) else {
            panic!("the fixture prices");
        };
        let stock_read = engine::momentum_score(&engine::compute_metrics(&fin))
            .expect("the short window has two closes");
        assert_eq!(
            out.sub_scores.momentum, stock_read,
            "fund momentum must be the stock read over the same window and band"
        );
        // Concretely: ~1% maps just above neutral, nowhere near the band's ceiling.
        assert!(
            out.sub_scores.momentum < 60.0,
            "momentum {} was scored off the deep history",
            out.sub_scores.momentum
        );
    }

    /// No short window → momentum imputes to the neutral 50, the stock path's own
    /// posture — the deep history never substitutes as the momentum input.
    #[test]
    fn fund_momentum_imputes_neutral_without_the_short_window() {
        let fin = CompanyFinancials {
            symbol: "VTI".to_string(),
            current_price: Some(300.0),
            price_history: vec![],
            daily_closes: vec![
                DatedValue {
                    date: "2022-01-03".into(),
                    value: 100.0,
                },
                DatedValue {
                    date: "2024-01-02".into(),
                    value: 200.0,
                },
                DatedValue {
                    date: "2026-07-15".into(),
                    value: 300.0,
                },
            ],
            ttm_dividends_per_share: Some(3.6),
            ..Default::default()
        };
        let inputs = FundEngineInputs {
            fund: &fund(),
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        let FundEngineVerdict::Priced(out) = analyze_fund(&inputs) else {
            panic!("the deep closes still carry the risk leg, so the fixture prices");
        };
        assert_eq!(out.sub_scores.momentum, 50.0);
    }

    #[test]
    fn bare_short_flags_inverse_names_but_not_duration_phrases() {
        // "ProShares Short S&P500" carries neither "-1x" nor "inverse" — bare
        // "short" must catch it; duration PHRASES ("Short-Term Bond", "Short
        // Duration") are maturity reads, not daily-reset vehicles.
        let mut sh = fund();
        sh.name = Some("ProShares Short S&P500".to_string());
        assert_eq!(classify(&sh).class, FundStrategyClass::LeveragedInverse);
        assert!(classify(&sh).structural_flag);
        // An inverse BOND fund must flag too — the suppression is phrase-shaped,
        // never a vocabulary veto ("treasury" anywhere must not excuse it).
        let mut tbf = fund();
        tbf.name = Some("ProShares Short 20+ Year Treasury".to_string());
        tbf.asset_class = Some("Fixed Income".to_string());
        assert_eq!(classify(&tbf).class, FundStrategyClass::LeveragedInverse);
        let mut bond = fund();
        bond.name = Some("iShares Short-Term Corporate Bond ETF".to_string());
        bond.asset_class = Some("Fixed Income".to_string());
        assert_eq!(classify(&bond).class, FundStrategyClass::Bond);
        let mut duration = fund();
        duration.name = Some("PIMCO Short Duration Municipal Income".to_string());
        duration.asset_class = Some("Fixed Income".to_string());
        assert_eq!(classify(&duration).class, FundStrategyClass::Bond);
        // "ultra" is ambiguous the same way: leverage (UltraPro / UltraShort)
        // flags, a duration phrase ("Ultra Short-Term Bond") suppresses.
        let mut ultra_duration = fund();
        ultra_duration.name = Some("iShares Ultra Short-Term Bond ETF".to_string());
        ultra_duration.asset_class = Some("Fixed Income".to_string());
        assert_eq!(classify(&ultra_duration).class, FundStrategyClass::Bond);
        let mut ultrashort = fund();
        ultrashort.name = Some("ProShares UltraShort 20+ Year Treasury".to_string());
        ultrashort.asset_class = Some("Fixed Income".to_string());
        assert_eq!(classify(&ultrashort).class, FundStrategyClass::LeveragedInverse);
        let mut ultra_long = fund();
        ultra_long.name = Some("ProShares Ultra S&P500".to_string());
        assert_eq!(classify(&ultra_long).class, FundStrategyClass::LeveragedInverse);
    }

    #[test]
    fn a_non_finite_weight_never_reaches_the_tilt_or_the_exposure_basis() {
        // Codex I7 / I16: the adapter drops such rows; the engine holds the
        // same line for any other producer, since both the readout's tilt and
        // the exposure basis persist the weight as a required float.
        let mut f = fund();
        f.sector_weights = vec![
            ("Technology".into(), f64::NAN),
            ("Energy".into(), 0.2),
            ("Utilities".into(), f64::INFINITY),
        ];
        assert_eq!(top_weights(&f.sector_weights), vec![("Energy".to_string(), 0.2)]);
        assert_eq!(exposure_basis(&f).top_sector, Some(("Energy".to_string(), 0.2)));
    }

    #[test]
    fn us_share_caps_at_one() {
        // A percent-served country set misread as fractions would report a
        // >100% US share straight into the ≥70% pricing guard — the cap bounds
        // the guard input at full-US.
        let mut misread = fund();
        misread.country_weights = vec![("United States".to_string(), 99.4)];
        let c = classify(&misread);
        assert_eq!(c.us_share, Some(1.0));
    }

    #[test]
    fn blend_averages_exchange_yields_not_pes() {
        let blended = blend_sector_pes(&snapshot());
        // Technology: yields 1/30 and 1/34 average to ~0.031373 → PE ≈ 31.875.
        let tech = blended.get("technology").unwrap();
        assert!((tech - 2.0 / (1.0 / 30.0 + 1.0 / 34.0)).abs() < 1e-9);
    }

    #[test]
    fn composite_renormalizes_over_covered_weight_only() {
        let mut blended = blend_sector_pes(&snapshot());
        blended.remove("energy"); // 20% of weight now uncovered
        let c = composite_yield(&weights(), &blended).unwrap();
        assert!((c.covered_share - 0.80).abs() < 1e-9);
        // The composite is over the covered weight, renormalized by it — the
        // uncovered slice neither reads as zero earnings nor extrapolates.
        let tech_pe = 2.0 / (1.0 / 30.0 + 1.0 / 34.0);
        let fin_pe = 2.0 / (1.0 / 14.0 + 1.0 / 16.0);
        let expected = (0.5 / tech_pe + 0.3 / fin_pe) / 0.8;
        assert!((c.yield_value - expected).abs() < 1e-12);
    }

    // ---- Panic posture: a hostile weight never reaches the sorts ----

    #[test]
    fn composite_skips_non_finite_weight_rows() {
        // A drifted string weight parses as NaN (or inf) at the adapter: the
        // row is not a weight — skipped, so the composite and its coverage read
        // exactly as over the finite rows.
        let blended = blend_sector_pes(&snapshot());
        let clean = composite_yield(&weights(), &blended).unwrap();
        let mut w = weights();
        w.push(("Technology".to_string(), f64::NAN));
        w.push(("Energy".to_string(), f64::INFINITY));
        let c = composite_yield(&w, &blended).unwrap();
        assert!((c.yield_value - clean.yield_value).abs() < 1e-12, "{c:?}");
        assert!((c.covered_share - clean.covered_share).abs() < 1e-12, "{c:?}");
        assert!(c.yield_value.is_finite());
    }

    #[test]
    fn a_non_finite_or_zero_composite_reads_as_no_composite() {
        let blended = blend_sector_pes(&snapshot());
        // Every weight non-finite: nothing covered — None, never NaN.
        let all_nan = vec![("Technology".to_string(), f64::NAN)];
        assert!(composite_yield(&all_nan, &blended).is_none());
        // Finite weights whose sum overflows: the yield collapses to zero — a
        // flat driver of zero prices nothing and its reciprocal sample is inf,
        // so the composite reads as absent.
        let overflow = vec![
            ("Technology".to_string(), f64::MAX),
            ("Financial Services".to_string(), f64::MAX),
        ];
        assert!(composite_yield(&overflow, &blended).is_none());
        // A blended P/E of zero (a subnormal print blown up by the yield
        // average): the composite is inf — absent, never a sample.
        let zero_pe: HashMap<String, f64> = [("technology".to_string(), 0.0)].into();
        assert!(composite_yield(&weights(), &zero_pe).is_none());
    }

    #[test]
    fn composite_history_over_non_finite_weights_keeps_finite_samples_only() {
        // The quarterly fixture backs every sample; a NaN weight row rides along
        // and changes nothing — every sample is finite and equals the clean
        // composite over the same prints.
        let clean = composite_yield_history(&weights(), &history(), as_of());
        assert_eq!(clean.len(), HISTORY_SAMPLE_QUARTERS);
        let mut w = weights();
        w.push(("Energy".to_string(), f64::NAN));
        let samples = composite_yield_history(&w, &history(), as_of());
        assert_eq!(samples.len(), HISTORY_SAMPLE_QUARTERS);
        for (s, c) in samples.iter().zip(&clean) {
            assert!(s.value.is_finite(), "{s:?}");
            assert_eq!(s.date, c.date);
            assert!((s.value - c.value).abs() < 1e-12, "{s:?} vs {c:?}");
        }
    }

    /// Every sector, both exchanges, one print per listed date at the given P/E.
    fn prints_at(dates: &[(&str, f64)]) -> HashMap<String, Vec<SectorPe>> {
        let mut map: HashMap<String, Vec<SectorPe>> = HashMap::new();
        for (sector, _) in weights() {
            for (date, pe) in dates {
                for exchange in ["NYSE", "NASDAQ"] {
                    map.entry(sector.to_ascii_lowercase()).or_default().push(SectorPe {
                        sector: sector.clone(),
                        exchange: exchange.to_string(),
                        date: date.to_string(),
                        pe: *pe,
                    });
                }
            }
        }
        map
    }

    fn history_floor_reason(history: &HashMap<String, Vec<SectorPe>>) -> Option<String> {
        let inputs = FundEngineInputs {
            fund: &fund(),
            financials: &financials(282.0),
            sector_pe: &snapshot(),
            sector_pe_history: history,
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::InsufficientEvidence(reason) => Some(reason),
            FundEngineVerdict::Priced(_) => None,
            other => panic!("the fixture is an equity fund: {other:?}"),
        }
    }

    #[test]
    fn one_stale_print_backs_no_history_sample_and_the_fund_abstains() {
        // Codex I2 (ruled 2026-08-28): the on-or-before select with no age bound
        // let a lone old print stand in for all twelve quarterly samples, pass the
        // eight-observation floor as twelve independent observations, score a
        // 0-or-100 percentile, and anchor twelve targets. A print backs only its
        // own quarter's sample, so one print is zero samples and the floor names
        // the count.
        let stale = prints_at(&[("2020-01-01", 20.0)]);
        assert!(composite_yield_history(&weights(), &stale, as_of()).is_empty());
        let reason = history_floor_reason(&stale).expect("abstains");
        assert!(
            reason.contains("only 0 constant-mix composite history samples")
                && reason.contains("distinct in-quarter"),
            "{reason}"
        );
    }

    #[test]
    fn a_sample_admits_only_prints_dated_within_its_own_quarter() {
        // The window is exclusive at the prior quarter end and inclusive at the
        // sample's own: a print ON a quarter end backs that quarter alone, the
        // next day's print backs the following quarter alone, and the same print
        // never backs two samples.
        let q6 = quarter_end_before(as_of(), 6);
        let q5 = quarter_end_before(as_of(), 5);
        let day_after_q6 = q6.succ_opt().unwrap();
        let history = prints_at(&[
            (&q6.format("%Y-%m-%d").to_string(), 20.0),
            (&day_after_q6.format("%Y-%m-%d").to_string(), 25.0),
        ]);
        let samples = composite_yield_history(&weights(), &history, as_of());
        let got: Vec<(String, f64)> = samples.iter().map(|s| (s.date.clone(), s.value)).collect();
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].0, q6.format("%Y-%m-%d").to_string());
        assert!((got[0].1 - 1.0 / 20.0).abs() < 1e-12, "{got:?}");
        assert_eq!(got[1].0, q5.format("%Y-%m-%d").to_string());
        assert!((got[1].1 - 1.0 / 25.0).abs() < 1e-12, "{got:?}");
    }

    #[test]
    fn a_dateless_print_never_qualifies_for_any_sample() {
        // Stored as served, a dateless row read as `""` — before every sample
        // date — and held an exchange's slot wherever no dated print qualified
        // (Codex I14). The sampler parses, so it is inadmissible everywhere: alone
        // it yields nothing, and beside the quarterly fixture it moves nothing
        // even at a wild P/E.
        let dateless = prints_at(&[("", 20.0)]);
        assert!(composite_yield_history(&weights(), &dateless, as_of()).is_empty());
        let clean = composite_yield_history(&weights(), &history(), as_of());
        let mut mixed = history();
        for (sector, prints) in prints_at(&[("", 1.0)]) {
            mixed.entry(sector).or_default().extend(prints);
        }
        let samples = composite_yield_history(&weights(), &mixed, as_of());
        assert_eq!(samples, clean);
    }

    #[test]
    fn a_non_padded_in_quarter_print_is_admitted_chronologically() {
        // The feed family's documented wire quirk: as source text "2026-6-15"
        // sorted after "2026-12-31", so the lexicographic sampler excluded it in
        // its own quarter and misselected it for later ones (Codex I14). Parsed,
        // it is the same print.
        let clean = composite_yield_history(&weights(), &history(), as_of());
        let mut unpadded = history();
        for prints in unpadded.values_mut() {
            for p in prints.iter_mut() {
                let d = NaiveDate::parse_from_str(&p.date, "%Y-%m-%d").unwrap();
                p.date = format!("{}-{}-{}", d.format("%Y"), d.format("%-m"), d.format("%-d"));
            }
        }
        assert!(unpadded.values().flatten().any(|p| p.date.len() < 10), "the fixture de-padded");
        let samples = composite_yield_history(&weights(), &unpadded, as_of());
        assert_eq!(samples, clean);
    }

    #[test]
    fn the_history_floor_counts_distinct_in_quarter_samples() {
        // Prints for the seven most recent sampled quarters abstain; eight price.
        let recent = |quarters: usize| {
            let cutoff = quarter_end_before(as_of(), quarters + 1);
            let mut map = history();
            for prints in map.values_mut() {
                prints.retain(|p| NaiveDate::parse_from_str(&p.date, "%Y-%m-%d").unwrap() > cutoff);
            }
            map
        };
        let seven = recent(7);
        assert_eq!(composite_yield_history(&weights(), &seven, as_of()).len(), 7);
        let reason = history_floor_reason(&seven).expect("seven abstains");
        assert!(reason.contains("only 7 constant-mix"), "{reason}");
        let eight = recent(8);
        assert_eq!(composite_yield_history(&weights(), &eight, as_of()).len(), 8);
        assert_eq!(history_floor_reason(&eight), None, "eight prices");
    }

    #[test]
    fn the_snapshot_blend_reads_no_date() {
        // The other sector-P/E consumer (Codex I14's separate pin): the snapshot
        // blend keys on sector alone, averaging the exchange prints, so the
        // shaper's canonical date render — or any date at all — changes nothing
        // there.
        let clean = blend_sector_pes(&snapshot());
        let redated: Vec<SectorPe> = snapshot()
            .into_iter()
            .enumerate()
            .map(|(i, row)| SectorPe {
                date: if i % 2 == 0 { "2026-7-15".to_string() } else { String::new() },
                ..row
            })
            .collect();
        assert_eq!(blend_sector_pes(&redated), clean);
    }

    #[test]
    fn an_unusable_quote_falls_to_a_usable_nav_never_masks_it() {
        // Codex I1 (ruled 2026-08-28): a served zero, negative, or non-finite
        // quote is no price. Beside a usable NAV the fund prices off the NAV —
        // the floor's `or(nav)` design keyed on usability — where a zero spot
        // used to abstain under the finite-target gate's misdescribed reason
        // and a negative one priced meaningless targets.
        for served in [0.0, -3.0, f64::INFINITY] {
            let mut fin = financials(282.0);
            fin.current_price = Some(served);
            let inputs = FundEngineInputs {
                fund: &fund(),
                financials: &fin,
                sector_pe: &snapshot(),
                sector_pe_history: &history(),
                rates: &rates(),
                as_of: as_of(),
            };
            match analyze_fund(&inputs) {
                FundEngineVerdict::Priced(out) => {
                    let basis = out
                        .quick_basis
                        .as_ref()
                        .expect("the priced fund records its basis");
                    assert_eq!(basis.spot, 280.0, "{served}: the NAV is the spot");
                    assert!(engine::price_targets_finite(&out.price_targets), "{served}");
                    // The rejected quote never re-enters as a premium — the
                    // read is absent (never the NAV-fallback's fabricated 0%,
                    // never a non-finite value the audit would persist as
                    // `null`) — Codex I1, round 1.
                    assert_eq!(out.metrics.nav_premium, None, "{served}");
                }
                other => panic!("{served}: expected pricing off the NAV, got {other:?}"),
            }
        }
    }

    #[test]
    fn no_usable_quote_or_nav_abstains_under_the_fund_floor() {
        // Neither leg usable — a negative or zero quote beside a zero NAV, or
        // no quote at all beside it — abstains under the fund analog's own
        // reason, never the finite-target gate's.
        let mut zero_nav = fund();
        zero_nav.nav = Some(0.0);
        for served in [Some(-3.0), Some(0.0), None] {
            let mut fin = financials(282.0);
            fin.current_price = served;
            let inputs = FundEngineInputs {
                fund: &zero_nav,
                financials: &fin,
                sector_pe: &snapshot(),
                sector_pe_history: &history(),
                rates: &rates(),
                as_of: as_of(),
            };
            match analyze_fund(&inputs) {
                FundEngineVerdict::InsufficientEvidence(reason) => {
                    assert!(
                        reason.contains("no usable current quote or NAV"),
                        "{served:?}: {reason}"
                    );
                }
                other => panic!("{served:?}: expected the fund floor, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_feed_extreme_quote_passes_the_floor_and_exits_at_the_finite_target_gate() {
        // The gate's fund-path pin, now that a non-finite quote lands at the
        // usability floor instead: `f64::MAX` is finite and positive, so it
        // passes the floor, and the flat driver (spot × composite yield) times
        // the anchored multiples overflows the scenario prices — the fund exits
        // as insufficient evidence under the gate's reason, never a `null`
        // target the store could not read back.
        let mut fin = financials(282.0);
        fin.current_price = Some(f64::MAX);
        let inputs = FundEngineInputs {
            fund: &fund(),
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("non-finite scenario pricing"), "{reason}");
            }
            other => panic!("expected the finite-target gate, got {other:?}"),
        }
    }

    #[test]
    fn a_sparse_served_weighting_reads_absolute_coverage_never_renormalized() {
        // One 1.4% sector row: renormalizing over the served rows' sum reported
        // 100% coverage and priced the whole fund off the sliver — absolute
        // coverage reads 1.4% and the ≥70% guard abstains downstream.
        let sparse = vec![("Technology".to_string(), 0.014)];
        let c = composite_yield(&sparse, &blend_sector_pes(&snapshot())).unwrap();
        assert!((c.covered_share - 0.014).abs() < 1e-12, "{c:?}");
        assert!(c.covered_share < PE_COVERAGE_GUARD);
    }

    #[test]
    fn classification_routes_the_unpriceable_classes_to_role_risk() {
        let mut leveraged = fund();
        leveraged.name = Some("Ultra 3x Daily Bull".to_string());
        assert!(classify(&leveraged).role_reason.is_some());
        assert!(classify(&leveraged).structural_flag);

        let mut bond = fund();
        bond.asset_class = Some("Fixed Income".to_string());
        let c = classify(&bond);
        assert_eq!(c.class, FundStrategyClass::Bond);
        assert!(c.role_reason.is_some());

        let mut intl = fund();
        intl.country_weights = vec![
            ("United States".to_string(), 0.40),
            ("Japan".to_string(), 0.60),
        ];
        let c = classify(&intl);
        assert!(c.role_reason.is_some(), "below the US-exposure guard");

        let mut weightless = fund();
        weightless.asset_class = Some("Equity".to_string());
        weightless.sector_weights = vec![];
        assert!(classify(&weightless).role_reason.is_some(), "the mutual-fund degrade");

        assert!(classify(&fund()).role_reason.is_none(), "the priced US equity fund");
    }

    #[test]
    fn priced_equity_fund_gets_the_fund_grade_contract_and_targets() {
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &fund(),
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::Priced(out) => {
                // The absent quality axis is neutral-imputed and the letter carries
                // the visible low-confidence marker.
                assert_eq!(out.sub_scores.quality, 50.0);
                assert!(out.low_confidence_grade);
                // Real valuation (vs-own-history percentile) and risk.
                assert!((0.0..=100.0).contains(&out.sub_scores.valuation));
                assert!((0.0..=100.0).contains(&out.sub_scores.risk));
                // Fund-form v2 targets: flat composite driver, versioned methodology.
                let tm = out.price_targets.twelve_month.as_ref().unwrap();
                assert!(tm.methodology.contains("fund exposure composite"), "{}", tm.methodology);
                assert!(out.target_meta.flat_driver);
                assert!(out.target_meta.rate_anchored, "12 history samples anchor");
                assert!(tm.bear <= tm.base && tm.base <= tm.bull);
                // Tier + hurdle exist on the priced branch.
                assert_ne!(
                    out.hurdle.state,
                    crate::portfolio::HurdleState::Unscorable
                );
                // Full coverage: recorded on the metrics, and no uncovered-share note.
                assert_eq!(out.metrics.composite_coverage, Some(1.0));
                assert!(out.tier_gaps.is_empty(), "{:?}", out.tier_gaps);
                // NAV premium is computed as context (282 / 280 − 1).
                assert!((out.metrics.nav_premium.unwrap() - (282.0 / 280.0 - 1.0)).abs() < 1e-9);
                assert_eq!(out.metrics.expense_ratio, Some(0.0003));
            }
            other => panic!("expected the priced fund branch, got {other:?}"),
        }
    }

    #[test]
    fn partial_coverage_grades_with_the_uncovered_share_reported() {
        // 80% of weight is P/E-usable (above the ≥70% guard): the fund grades, the
        // coverage rides the metrics, and the uncovered slice is a recorded note —
        // reported beside the read, never averaged in.
        let mut partial = fund();
        partial.sector_weights = vec![
            ("Technology".to_string(), 0.50),
            ("Financial Services".to_string(), 0.30),
            ("Utilities".to_string(), 0.20), // not in the snapshot or history
        ];
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &partial,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::Priced(out) => {
                assert!((out.metrics.composite_coverage.unwrap() - 0.80).abs() < 1e-9);
                assert!(
                    out.tier_gaps.iter().any(|g| g.contains("composite P/E coverage")),
                    "the uncovered share must be a recorded note: {:?}",
                    out.tier_gaps
                );
            }
            other => panic!("expected the priced branch, got {other:?}"),
        }

    }

    #[test]
    fn missing_floor_bearing_fund_inputs_abstain_on_the_priced_branch() {
        // The expense ratio is a floor-bearing fund-analog input — absent, the fund
        // abstains rather than pricing without it (`docs/portfolio-analysis.md`
        // §Evidence floor).
        let mut no_er = fund();
        no_er.expense_ratio = None;
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &no_er,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("expense ratio"), "{reason}");
            }
            other => panic!("expected the floor abstention, got {other:?}"),
        }

        // Missing country weightings leave the ≥ 70% US-exposure guard unverifiable —
        // floor-bearing weighting coverage on the exposure-priced branch, so the fund
        // abstains rather than pricing on an assumed premise.
        let mut no_countries = fund();
        no_countries.country_weights = vec![];
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &no_countries,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("US-exposure"), "{reason}");
            }
            other => panic!("expected the floor abstention, got {other:?}"),
        }

        // The floor checks never pre-empt the structural routing: a bond fund missing
        // its expense ratio is still `role_risk_only`, not an abstention.
        let mut bond = fund();
        bond.asset_class = Some("Fixed Income".to_string());
        bond.expense_ratio = None;
        bond.sector_weights = vec![];
        let fin = financials(100.0);
        let inputs = FundEngineInputs {
            fund: &bond,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        assert!(matches!(
            analyze_fund(&inputs),
            FundEngineVerdict::RoleRiskOnly(_)
        ));
    }

    #[test]
    fn option_overlay_funds_carry_the_structural_flag_and_stay_priced() {
        // An option-overlay fund is not in the unpriceable list — it prices, carries
        // the deterministic path-dependency flag, and the flag bars the Low tier
        // without forcing High (`docs/portfolio-analysis.md` §Asset eligibility,
        // §Starting parameters).
        let mut overlay = fund();
        overlay.name = Some("US Equity Covered Call ETF".to_string());
        let c = classify(&overlay);
        assert!(c.structural_flag);
        assert!(c.role_reason.is_none(), "overlay funds still price");

        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &overlay,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::Priced(out) => {
                assert!(
                    out.tier_gaps.iter().any(|g| g.contains("option-overlay")),
                    "the flag must be a recorded note: {:?}",
                    out.tier_gaps
                );
                // The fixture's realized volatility is low — without the flag this
                // fund would read Low; the overlay bars it.
                assert_eq!(out.risk_tier, crate::portfolio::RiskTier::Medium);
                // The classification rides the priced output — card-visible, not
                // just an audit note.
                assert!(out.structural_flag);
                assert_eq!(out.fund_class_label.as_deref(), Some("US equity fund"));
            }
            other => panic!("expected the priced branch, got {other:?}"),
        }

        // The unflagged control: same fund without overlay naming reads Low.
        let plain = fund();
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &plain,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::Priced(out) => {
                assert_eq!(out.risk_tier, crate::portfolio::RiskTier::Low);
            }
            other => panic!("expected the priced branch, got {other:?}"),
        }
    }

    #[test]
    fn coverage_below_the_guard_abstains_rather_than_lettering_a_sliver() {
        let mut thin = fund();
        // Only 50% of weight is in sectors the snapshot prices.
        thin.sector_weights = vec![
            ("Technology".to_string(), 0.50),
            ("Utilities".to_string(), 0.50), // not in the snapshot
        ];
        let fin = financials(282.0);
        let inputs = FundEngineInputs {
            fund: &thin,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("coverage"), "{reason}");
            }
            other => panic!("expected the coverage abstention, got {other:?}"),
        }
    }

    #[test]
    fn bond_fund_returns_the_role_risk_readout() {
        let mut bond = fund();
        bond.asset_class = Some("Fixed Income".to_string());
        bond.sector_weights = vec![];
        let fin = financials(100.0);
        let inputs = FundEngineInputs {
            fund: &bond,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::RoleRiskOnly(r) => {
                assert_eq!(r.class_label, "bond fund");
                assert!(!r.structural_flag);
                assert!(r.observable_risk.is_some(), "vol from price history");
                assert!(!r.exposure_tilt.is_empty(), "country tilt stands in");
                assert!(r.evidence_gaps.iter().any(|g| g.contains("duration")));
            }
            other => panic!("expected role_risk_only, got {other:?}"),
        }
    }

    #[test]
    fn missing_quote_and_missing_info_abstain_under_the_fund_floor() {
        let mut fin = financials(282.0);
        fin.current_price = None;
        let mut no_nav = fund();
        no_nav.nav = None;
        let inputs = FundEngineInputs {
            fund: &no_nav,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        assert!(matches!(
            analyze_fund(&inputs),
            FundEngineVerdict::InsufficientEvidence(_)
        ));

        let bare = FundData { symbol: "XXX".into(), ..Default::default() };
        let fin = financials(50.0);
        let inputs = FundEngineInputs {
            fund: &bare,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::InsufficientEvidence(reason) => {
                assert!(reason.contains("etf/info"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn quarter_end_walkback_is_correct() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 16).unwrap();
        assert_eq!(quarter_end_before(d, 1).to_string(), "2026-06-30");
        assert_eq!(quarter_end_before(d, 2).to_string(), "2026-03-31");
        assert_eq!(quarter_end_before(d, 5).to_string(), "2025-06-30");
        let jan = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        assert_eq!(quarter_end_before(jan, 1).to_string(), "2025-12-31");
    }

    /// The real closed-end arrival shape (probe 2026-08-21): `etf/info` serves
    /// `[]`, so every fund-metadata field is `None` and only the profile carries
    /// the structure signal.
    fn cef_fund() -> FundData {
        FundData {
            symbol: "PDI".to_string(),
            profile_is_fund: Some(true),
            profile_description: Some(
                "The PIMCO Dynamic Income Fund (PDI) is a closed-end mutual fund \
                 specializing in fixed-income investments."
                    .to_string(),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn closed_end_detection_requires_both_profile_legs() {
        assert!(is_closed_end(&cef_fund()));
        // GAB's wire form ("closed ended") matches through the spaced fragment.
        let mut gab = cef_fund();
        gab.profile_description =
            Some("a closed ended equity mutual fund launched by GAMCO".to_string());
        assert!(is_closed_end(&gab));
        // The flag without the text never guesses a CEF — `isFund` is FMP's flag
        // for open-end mutual funds too.
        let mut open_end = cef_fund();
        open_end.profile_description = Some("an open-end index mutual fund".to_string());
        assert!(!is_closed_end(&open_end));
        // The text without the flag never guesses either (manager boilerplate).
        let mut unflagged = cef_fund();
        unflagged.profile_is_fund = Some(false);
        assert!(!is_closed_end(&unflagged));
        // No profile at all reads not-a-CEF, never a guess.
        assert!(!is_closed_end(&FundData::default()));
    }

    #[test]
    fn nav_premium_reads_the_market_price_only() {
        let prem = nav_premium_read(Some(282.0), Some(280.0)).unwrap();
        assert!((prem - (282.0 / 280.0 - 1.0)).abs() < 1e-12);
        // No market quote is a gap — never a fabricated 0% off the NAV-fallback
        // spot the engine floor otherwise substitutes.
        assert_eq!(nav_premium_read(None, Some(280.0)), None);
        assert_eq!(nav_premium_read(Some(282.0), None), None);
        assert_eq!(nav_premium_read(Some(282.0), Some(0.0)), None);
        // Both legs read through the usability test, and the read itself must
        // be finite: an unusable quote (zero, negative, non-finite) or NAV is
        // no premium, and a finite pair whose quotient overflows is none either
        // — never an `inf` the audit would persist as `null` (Codex I1, round 1).
        assert_eq!(nav_premium_read(Some(0.0), Some(280.0)), None);
        assert_eq!(nav_premium_read(Some(-3.0), Some(280.0)), None);
        assert_eq!(nav_premium_read(Some(f64::INFINITY), Some(280.0)), None);
        assert_eq!(nav_premium_read(Some(f64::NAN), Some(280.0)), None);
        assert_eq!(nav_premium_read(Some(282.0), Some(-1.0)), None);
        assert_eq!(nav_premium_read(Some(282.0), Some(f64::INFINITY)), None);
        assert_eq!(nav_premium_read(Some(f64::MAX), Some(1e-300)), None);
    }

    #[test]
    fn classify_marks_the_closed_end_structure_orthogonally() {
        // Empty `etf/info` + the profile signal: the one branch a real CEF
        // reaches — the card says what IS known, not "unresolved strategy class".
        let c = classify(&cef_fund());
        assert!(c.is_cef);
        assert_eq!(c.class, FundStrategyClass::Unknown);
        assert_eq!(c.class_label, "closed-end fund");
        assert!(c.role_reason.as_deref().is_some_and(|r| r.contains("closed-end")));
        // A CEF whose surface someday serves a class string keeps its routing —
        // the structure rides the label, orthogonal like the overlay flag.
        let mut bond_cef = cef_fund();
        bond_cef.asset_class = Some("Fixed Income".to_string());
        let c = classify(&bond_cef);
        assert_eq!(c.class, FundStrategyClass::Bond);
        assert!(c.is_cef);
        assert_eq!(c.class_label, "bond fund (closed-end)");
    }

    #[test]
    fn a_cef_with_empty_etf_info_takes_the_role_risk_readout_not_abstention() {
        let cef = cef_fund();
        let fin = financials(15.12);
        let inputs = FundEngineInputs {
            fund: &cef,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::RoleRiskOnly(r) => {
                assert_eq!(r.class_label, "closed-end fund");
                assert!(r.is_cef);
                // No NAV on the surface: the read is the named gap, never 0%.
                assert_eq!(r.nav_premium, None);
                assert!(
                    r.evidence_gaps.iter().any(|g| g.contains("etf/info) is empty")),
                    "{:?}",
                    r.evidence_gaps
                );
                assert!(
                    r.evidence_gaps.iter().any(|g| g.contains("price-vs-NAV unavailable")),
                    "{:?}",
                    r.evidence_gaps
                );
            }
            other => panic!("expected the role/risk readout, got {other:?}"),
        }
    }

    #[test]
    fn a_cef_missing_or_unusable_quote_names_the_quote_not_the_nav() {
        // The spot floor accepts the NAV as the price fallback — a zero quote
        // is unusable and falls to it (Codex I1) — so the run proceeds; but
        // the premium is honestly absent, and the gap must name the unusable
        // market quote rather than claim the present NAV is absent
        // (Codex 2026-08-21 rounds 3–4, finding 2).
        let run = |current_price: Option<f64>| {
            let mut cef = cef_fund();
            cef.name = Some("PIMCO Dynamic Income Fund".to_string());
            cef.nav = Some(14.0);
            let mut fin = financials(15.12);
            fin.current_price = current_price;
            let inputs = FundEngineInputs {
                fund: &cef,
                financials: &fin,
                sector_pe: &snapshot(),
                sector_pe_history: &history(),
                rates: &rates(),
                as_of: as_of(),
            };
            match analyze_fund(&inputs) {
                FundEngineVerdict::RoleRiskOnly(r) => *r,
                other => panic!("expected the role/risk readout, got {other:?}"),
            }
        };
        for price in [None, Some(0.0)] {
            let r = run(price);
            assert_eq!(r.nav_premium, None);
            assert!(
                r.evidence_gaps.iter().any(
                    |g| g.contains("no usable market quote to read against the reported NAV")
                ),
                "price {price:?}: {:?}",
                r.evidence_gaps
            );
        }
    }

    #[test]
    fn a_cef_gap_cause_reads_usability_on_both_legs_and_names_a_non_finite_read() {
        // Codex I1, round 2: an unusable NAV beside a usable quote names the
        // NAV (a raw `nav > 0` test called an infinite NAV present and blamed
        // the quote), and a usable pair whose quotient overflows names the
        // read rather than a leg that is not missing.
        let run = |price: Option<f64>, nav: Option<f64>| {
            let mut cef = cef_fund();
            cef.name = Some("PIMCO Dynamic Income Fund".to_string());
            cef.nav = nav;
            let mut fin = financials(15.12);
            fin.current_price = price;
            let inputs = FundEngineInputs {
                fund: &cef,
                financials: &fin,
                sector_pe: &snapshot(),
                sector_pe_history: &history(),
                rates: &rates(),
                as_of: as_of(),
            };
            match analyze_fund(&inputs) {
                FundEngineVerdict::RoleRiskOnly(r) => *r,
                other => panic!("expected the role/risk readout, got {other:?}"),
            }
        };
        let r = run(Some(15.12), Some(f64::INFINITY));
        assert_eq!(r.nav_premium, None);
        assert!(
            r.evidence_gaps.iter().any(|g| g.contains("no usable NAV")),
            "{:?}",
            r.evidence_gaps
        );
        let r = run(Some(f64::MAX), Some(1e-300));
        assert_eq!(r.nav_premium, None);
        assert!(
            r.evidence_gaps
                .iter()
                .any(|g| g.contains("did not come out finite")),
            "{:?}",
            r.evidence_gaps
        );
    }

    #[test]
    fn a_cef_with_a_served_nav_carries_the_premium_and_drops_the_gap() {
        // The seam's live shape if the surface ever serves a CEF NAV: the read
        // renders, and neither CEF gap is pushed.
        let mut cef = cef_fund();
        cef.name = Some("PIMCO Dynamic Income Fund".to_string());
        cef.nav = Some(14.0);
        let fin = financials(15.12);
        let inputs = FundEngineInputs {
            fund: &cef,
            financials: &fin,
            sector_pe: &snapshot(),
            sector_pe_history: &history(),
            rates: &rates(),
            as_of: as_of(),
        };
        match analyze_fund(&inputs) {
            FundEngineVerdict::RoleRiskOnly(r) => {
                assert!(r.is_cef);
                let prem = r.nav_premium.expect("premium from market price vs NAV");
                assert!((prem - (15.12 / 14.0 - 1.0)).abs() < 1e-12);
                assert!(
                    !r.evidence_gaps.iter().any(|g| g.contains("price-vs-NAV unavailable")),
                    "{:?}",
                    r.evidence_gaps
                );
            }
            other => panic!("expected the role/risk readout, got {other:?}"),
        }
    }
}
