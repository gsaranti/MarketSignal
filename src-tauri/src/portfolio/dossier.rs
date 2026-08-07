//! Deterministic dossier assembly (`docs/portfolio-analysis.md` §The per-holding
//! pipeline, step 1; `docs/local-models.md §Context-memory discipline` — "Deterministic
//! packet assembly"). The application layer builds a holding's evidence packet so the
//! model reasons over bounded, structured context rather than gathering it itself.
//!
//! Two seams keep this honest: the **financials merge** unifies the FMP per-company
//! pull with keyless SEC EDGAR facts and *derives* valuation multiples from market cap
//! plus statement lines (compute, don't guess); and the **house view** loads the
//! Market Signal Report deterministically — the latest report's Thesis / Investment
//! Strategy / Forward Outlook sections plus the recent report summaries — never by
//! vector-searching the report's memory partition. The prior run's verdict for this
//! holding rides in for the continuity check.

use std::path::Path;

use rusqlite::Connection;

use crate::agent::ReportSummary;
use crate::portfolio::engine::CompanyFinancials;
use crate::portfolio::{
    HoldingVerdict, InvestorProfile, OptionsSignal, PositionDelta, HOUSE_VIEW_RECENT_REPORTS,
};
use crate::schwab::{OptionChain, Position};
use crate::sec::CompanyFacts;
use crate::storage;

/// The Market Signal house view loaded as a read-only shared input
/// (`docs/portfolio-analysis.md`). It enters deterministically — recent report
/// summaries plus the latest report's relevant prose sections — never via the
/// report's vector memory (which a local job cannot read anyway: different namespace
/// and embedder, see `crate::vector_memory::MemoryNamespace`).
#[derive(Debug, Clone, Default)]
pub struct HouseView {
    pub recent_summaries: Vec<ReportSummary>,
    /// The latest report's Thesis / Investment Strategy / Forward Outlook prose,
    /// concatenated; `None` when no report exists or none could be read.
    pub latest_sections: Option<String>,
}

/// A holding's complete evidence packet, assembled deterministically. The pipeline's
/// model stages read only this (plus the engine's computed numbers), so interpretation
/// reasons over evidence, not over a gathering transcript.
#[derive(Debug, Clone)]
pub struct HoldingDossier {
    pub position: Position,
    /// How this position changed since the prior run (the Step-4 holdings diff), so the
    /// verdict reasons over what the user did with it (`docs/portfolio-analysis.md`
    /// §Holdings change tracking).
    pub position_delta: PositionDelta,
    pub financials: CompanyFinancials,
    pub options_signal: OptionsSignal,
    pub profile: InvestorProfile,
    pub house_view: HouseView,
    /// The fund half (metadata + the sector-P/E surface) for an ETF / mutual-fund
    /// holding — the reduced path's input (`docs/portfolio-analysis.md` §Asset
    /// eligibility); `None` for a stock.
    pub fund: Option<crate::portfolio::fund::FundContext>,
    /// The prior run's verdict for this holding (continuity input), or `None` on a
    /// holding the job has not seen before ("new holding").
    pub prior_verdict: Option<HoldingVerdict>,
    /// The prior verdict's **effective analysis vintage** — its `analyzed_at`,
    /// else its run's `created_at` — the retrospective block's "since" anchor and
    /// the anchor-close bridge's session key (`docs/portfolio-analysis.md` §The
    /// holding verdict, the two-arm contract). A selective carry keeps the
    /// original authoring vintage, so this date and `prior_spot` stay paired
    /// (Codex round 2, finding 2). `None` on a debut.
    pub prior_vintage: Option<String>,
    /// The prior read's authoring-time spot, **on the prior read's price basis** —
    /// the anchor-close bridge's authoring leg for re-basing the prior authored
    /// targets. `None` on a debut or a prior audit without a quick-check basis.
    pub prior_spot: Option<f64>,
    /// The prior run's matured outcome-window lines for this symbol (deterministic,
    /// engine-computed) — the scored ground the retrospective reads against, where
    /// any windows have matured. Empty on a debut or before any window matures.
    pub prior_matured_notes: Vec<String>,
    /// The grade-band parameter version the prior verdict's letter was computed under
    /// (from the prior run's audit row; `None` = a pre-stamp run, i.e. the v1 bands).
    /// Meaningful only beside `prior_verdict` — the interpretation prompt compares it
    /// against the current [`crate::portfolio::engine::GRADE_PARAMETER_VERSION`] so a
    /// band recalibration's letter move is attributed to the retune, not to evidence.
    pub prior_grade_parameter_version: Option<String>,
    /// The prior run's pre-profit overlay record (from the audit row) — the
    /// period-keyed observation history accumulates through it
    /// (`docs/portfolio-analysis.md` §Starting parameters). `None` on a debut, a
    /// pre-overlay run, or a fund.
    pub prior_pre_profit: Option<crate::portfolio::pre_profit::PreProfitOverlay>,
    /// The loop-time listing-resolution guard's outcome for a stock
    /// (`docs/portfolio-analysis.md` §Asset eligibility) — computed at gather time,
    /// routed by `analyze_holding` beside the eligibility gates. `None` on a fund
    /// (the guard is stocks-only); a stock always carries `Some` — offline stubs
    /// ride the trait default's `Unverified`, which proceeds with a recorded
    /// degraded input, never a terminal outcome.
    pub listing: Option<crate::portfolio::listing::ListingResolution>,
    /// The data sources that contributed, for the run's audit record.
    pub sources: Vec<String>,
}

/// The prior run's carry-over for one holding, read off the latest persisted run:
/// the verdict (continuity input) plus the audit-row legs the next pass consumes.
#[derive(Debug, Clone)]
pub struct PriorHolding {
    pub verdict: HoldingVerdict,
    /// The grade-band parameter version the prior letter was computed under
    /// (`None` = a pre-stamp run, i.e. the v1 bands).
    pub grade_parameter_version: Option<String>,
    /// The prior pre-profit overlay record — the observation history's carry path.
    pub pre_profit: Option<crate::portfolio::pre_profit::PreProfitOverlay>,
    /// The verdict's **effective analysis vintage** (`analyzed_at`, else the
    /// prior run's `created_at`) — the retrospective's "since" anchor. A
    /// selective carry keeps the original vintage, so this date stays paired
    /// with the carried audit's authoring spot (Codex round 2, finding 2).
    pub vintage: String,
    /// The prior run's authoring-time spot (its audit's quick-check basis print) —
    /// the base the retrospective's realized price move computes against. `None`
    /// where the prior audit carried no basis.
    pub spot: Option<f64>,
    /// The prior run's matured outcome-window lines for this symbol.
    pub matured_notes: Vec<String>,
}

impl HoldingDossier {
    /// The prior run's thesis ledger for this holding — it rides the prior verdict
    /// (`docs/portfolio-analysis.md` §The position thesis ledger: read at dossier
    /// assembly, re-evaluated and rewritten each run). `None` on a debut or a
    /// pre-ledger prior run.
    pub fn prior_ledger(&self) -> Option<&crate::portfolio::ThesisLedger> {
        self.prior_verdict
            .as_ref()
            .and_then(|v| v.thesis_ledger.as_ref())
    }
}

/// Adopt the **TTM statement basis** where the quarterly income prints support it
/// (`docs/portfolio-analysis.md` §Starting parameters — the grade-band slice's F5
/// closure): the four newest quarters sum to TTM revenue / net income / gross
/// profit, and quarters five through eight to the prior-TTM revenue the growth read
/// compares against. **One statement basis per holding**: a ratio's numerator and
/// denominator must share a period basis, and `revenue` denominates several, so the
/// margin / growth / multiple family either adopts TTM wholesale or stays on the
/// SEC annual fill — never a mix (an annual gross profit over a TTM revenue would
/// fabricate a margin). Adoption requires revenue and net income on all four newest
/// quarters — the lines the margins and multiples cannot do without; gross profit
/// sums where every quarter carries it (or derives per-quarter from cost of
/// revenue), else stays an honest gap even when SEC has an annual print. Returns
/// whether the basis was adopted, so the SEC merge confines itself to fallback.
pub fn apply_ttm_statement_basis(fin: &mut CompanyFinancials) -> bool {
    let adopted = adopt_ttm_statement_basis(fin);
    // Stamp WHICH basis the values now stand on, at the shared choke point every
    // statement-consuming path already passes through, so no producer can set the
    // levels without recording their basis. The ledger evaluation reads it to detect
    // a basis change; it alters no value.
    //
    // No quarterly rows at all means FMP alone supports no statement basis, which is
    // distinct from a resolved fallback: `Annual` asserts a same-concept annual window
    // is what the levels came from.
    //
    // This is what FMP's own pull supports. Where a SEC merge follows
    // ([`merge_financials`]) it **refines** this: a zero-row quarterly response whose
    // levels are then filled from SEC annual facts is on the annual basis, and saying
    // `None` there would exempt it from the ledger's basis-continuity gate — the exact
    // fabricated crossing that gate exists to stop. The refinement lives at the merge
    // because only the merge knows what finally supplied the levels; a caller with no
    // merge (the quick check's `statements_refresh`) is correctly described here.
    fin.statement_basis = if adopted {
        Some(crate::portfolio::StatementBasis::Ttm)
    } else if fin.quarterly_income.is_empty() {
        None
    } else {
        Some(crate::portfolio::StatementBasis::Annual)
    };
    adopted
}

/// The adoption decision itself — `true` when the four newest quarters are a usable
/// TTM window, `false` onto the annual-fallback path.
fn adopt_ttm_statement_basis(fin: &mut CompanyFinancials) -> bool {
    // Canonicalize the statement rows **in place** first — newest-first with the
    // latest filing winning a duplicated period (`engine::canonicalize_statements`,
    // the shared policy): a restatement served twice must resolve to the restated
    // print, never to wire order, or the TTM basis (and with it the letter) would
    // depend on response ordering. Every statement-consuming path passes here
    // before any engine read, so the driver ladder's trailing prints and the
    // anchor windows inherit the same canonical order.
    crate::portfolio::engine::canonicalize_statements(fin);
    let rows = &fin.quarterly_income;

    fn sum4(
        rows: &[crate::portfolio::engine::QuarterlyIncomeRow],
        get: impl Fn(&crate::portfolio::engine::QuarterlyIncomeRow) -> Option<f64>,
    ) -> Option<f64> {
        if rows.len() < 4 {
            return None;
        }
        rows[..4].iter().map(get).sum()
    }
    // The four newest rows are a TTM only when they are consecutive quarters —
    // a feed gap (missed quarter, fiscal transition) would silently sum a
    // >12-month span into every level-vs-market-cap multiple. Non-contiguous
    // reads fail adoption onto the existing annual-fallback path.
    if !crate::portfolio::engine::quarters_contiguous(
        rows.iter().take(4).map(|r| r.period_end.as_str()),
    ) {
        return false;
    }
    let ttm_revenue = sum4(rows, |r| r.revenue);
    let ttm_net_income = sum4(rows, |r| r.net_income);
    let (Some(_), Some(_)) = (ttm_revenue, ttm_net_income) else {
        return false;
    };
    // Per-quarter: each quarter contributes its reported gross line, or derives
    // its own from revenue − cost of revenue — mixing reported and derived
    // quarters is fine (same economic line per quarter), a quarter with neither
    // gaps the whole sum.
    let ttm_gross_profit = sum4(rows, |r| {
        r.gross_profit
            .or_else(|| Some(r.revenue? - r.cost_of_revenue?))
    });
    // The prior-year window needs the full eight-row run contiguous — a gap
    // anywhere across it shifts the YoY comparison by a quarter, so growth
    // gaps (`None`) rather than reading a misaligned window. The TTM basis
    // itself stands: its own four rows were verified above.
    let prior = if rows.len() >= 8
        && crate::portfolio::engine::quarters_contiguous(
            rows.iter().take(8).map(|r| r.period_end.as_str()),
        ) {
        &rows[4..8]
    } else {
        &[][..]
    };
    let prior_revenue = sum4(prior, |r| r.revenue);

    fin.revenue = ttm_revenue;
    fin.net_income = ttm_net_income;
    fin.gross_profit = ttm_gross_profit;
    fin.revenue_prior = prior_revenue;
    true
}

/// Merge the keyless SEC EDGAR facts into the FMP per-company financials and derive
/// the valuation multiples from market cap plus statement lines. On the annual
/// fallback basis SEC fills the statement fields FMP left empty (revenue and its
/// prior-year print, gross profit, net income) — a missing field stays a gap rather
/// than a fabricated level; when the TTM basis was adopted
/// ([`apply_ttm_statement_basis`]) those fills are skipped wholesale so the two
/// period bases never mix inside one ratio. Equity fills either way (a
/// balance-sheet instant, not a flow line — the FMP quarterly balance sheet is
/// preferred upstream, SEC the fallback), and the multiples are computed only when
/// both market cap and the denominator are present.
pub fn merge_financials(
    mut fmp: CompanyFinancials,
    sec: &CompanyFacts,
    ttm_statement_basis: bool,
) -> CompanyFinancials {
    let fill = |dst: &mut Option<f64>, src: Option<i64>| {
        if dst.is_none() {
            if let Some(v) = src {
                *dst = Some(v as f64);
            }
        }
    };
    if !ttm_statement_basis {
        fill(&mut fmp.revenue, sec.revenue);
        fill(&mut fmp.revenue_prior, sec.revenue_prior);
        fill(&mut fmp.gross_profit, sec.gross_profit);
        fill(&mut fmp.net_income, sec.net_income);
    }
    fill(&mut fmp.total_equity, sec.stockholders_equity);

    // Refine the basis stamp now that the fills have run — see
    // [`apply_ttm_statement_basis`]. An adopted TTM window stands; otherwise the basis
    // is `Annual` whenever any statement-derived level is actually present, however it
    // arrived, and `None` only when there are none at all (a fund, or a holding whose
    // statement surface resolved to nothing). Stamping `None` while carrying
    // annual-derived levels would slip them past the basis-continuity gate.
    if !ttm_statement_basis {
        let has_statement_level = [
            fmp.revenue,
            fmp.revenue_prior,
            fmp.gross_profit,
            fmp.net_income,
            fmp.total_equity,
        ]
        .iter()
        .any(Option::is_some);
        fmp.statement_basis = has_statement_level.then_some(crate::portfolio::StatementBasis::Annual);
    }

    // Derive multiples from market cap + fundamentals when FMP didn't supply them.
    let derive = |num: Option<f64>, den: Option<f64>| match (num, den) {
        (Some(n), Some(d)) if d > 0.0 => Some(n / d),
        _ => None,
    };
    // The P/E derive is SIGNED (denominator ≠ 0, not > 0): a loss-maker must
    // produce a negative P/E so the engine's fixed "a loss-maker is never
    // cheap" valuation guard is reachable — a `None` here silently excused
    // every loss-maker from the penalty (the grade-v2.1 boundary; the
    // band-recalibration NOTE attributes the letter shifts). P/S and P/B keep
    // the positive-denominator derive: negative revenue does not occur, and a
    // negative-equity P/B has no engine read (risk carries the negative-D/E
    // guard instead).
    if fmp.pe_ratio.is_none() {
        fmp.pe_ratio = match (fmp.market_cap, fmp.net_income) {
            (Some(n), Some(d)) if d != 0.0 => Some(n / d),
            _ => None,
        };
    }
    if fmp.ps_ratio.is_none() {
        fmp.ps_ratio = derive(fmp.market_cap, fmp.revenue);
    }
    if fmp.pb_ratio.is_none() {
        fmp.pb_ratio = derive(fmp.market_cap, fmp.total_equity);
    }
    fmp
}

/// Assemble the dossier from already-fetched pieces. Pure: the network fetches (FMP,
/// SEC, the Schwab chain) happen in the job, which hands the results here so this
/// assembly stays deterministic and testable. The options signal is computed from the
/// chain when present; absent, it is empty (and the grade is unaffected, since the
/// signal never feeds it).
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    position: Position,
    position_delta: PositionDelta,
    fmp_financials: CompanyFinancials,
    sec_facts: &CompanyFacts,
    chain: Option<&OptionChain>,
    profile: InvestorProfile,
    house_view: HouseView,
    fund: Option<crate::portfolio::fund::FundContext>,
    prior: Option<PriorHolding>,
    listing: Option<crate::portfolio::listing::ListingResolution>,
) -> HoldingDossier {
    let (
        prior_verdict,
        prior_grade_parameter_version,
        prior_pre_profit,
        prior_vintage,
        prior_spot,
        prior_matured_notes,
    ) = match prior {
        Some(p) => (
            Some(p.verdict),
            p.grade_parameter_version,
            p.pre_profit,
            Some(p.vintage),
            p.spot,
            p.matured_notes,
        ),
        None => (None, None, None, None, None, Vec::new()),
    };
    let mut fmp_financials = fmp_financials;
    let ttm_basis = apply_ttm_statement_basis(&mut fmp_financials);
    let financials = merge_financials(fmp_financials, sec_facts, ttm_basis);
    let options_signal = chain
        .map(crate::portfolio::engine::options_signal)
        .unwrap_or(OptionsSignal {
            put_call_volume: None,
            put_call_open_interest: None,
            implied_volatility: None,
            iv_skew: None,
        });

    // A guard-terminal holding never consulted the statement surface — its audit
    // must not claim it; the profile identity read is the evidence that actually
    // drove the verdict (`docs/portfolio-analysis.md` §Asset eligibility).
    let guard_terminal = matches!(
        &listing,
        Some(
            crate::portfolio::listing::ListingResolution::Unresolved
                | crate::portfolio::listing::ListingResolution::NonUs { .. }
                | crate::portfolio::listing::ListingResolution::Conflict { .. }
        )
    );
    let mut sources = if guard_terminal {
        vec!["FMP company profile (listing-resolution guard)".to_string()]
    } else {
        vec!["FMP company financials".to_string()]
    };
    if ttm_basis {
        sources.push("FMP TTM statement basis (four-quarter sums)".to_string());
    }
    if !sec_facts.is_empty() {
        sources.push("SEC EDGAR company facts".to_string());
    }
    if chain.is_some() {
        sources.push("Schwab option chain".to_string());
    }
    if fund.is_some() {
        sources.push("FMP fund metadata (etf/info + weightings + sector P/E)".to_string());
    }
    if !house_view.recent_summaries.is_empty() || house_view.latest_sections.is_some() {
        sources.push("Market Signal Report (house view)".to_string());
    }

    HoldingDossier {
        position,
        position_delta,
        financials,
        options_signal,
        profile,
        house_view,
        fund,
        prior_verdict,
        prior_vintage,
        prior_spot,
        prior_matured_notes,
        prior_grade_parameter_version,
        prior_pre_profit,
        listing,
        sources,
    }
}

/// The house-view freshness window, in days — the pinned default of
/// `docs/portfolio-workflow.md` §Step 5: a latest report older than this is
/// omitted whole and recorded as a gap rather than fed as current (a month-old
/// thesis is not today's; a stale input is absent, not current).
pub const HOUSE_VIEW_MAX_AGE_DAYS: i64 = 7;

/// Load the Market Signal house view deterministically: the most recent
/// [`HOUSE_VIEW_RECENT_REPORTS`] report summaries and the latest report's relevant
/// prose sections. Fail-soft — an unreadable DB or missing Markdown degrades to a
/// thinner house view, never an error (the holding still grades on its fundamentals).
/// Freshness-gated: a latest report older than [`HOUSE_VIEW_MAX_AGE_DAYS`] drops
/// the whole view — summaries included — and the second return reports the
/// omission so the run's data health records the gap.
pub fn load_house_view(
    conn: &Connection,
    reports_dir: &Path,
    today: chrono::NaiveDate,
) -> (HouseView, bool) {
    let with_paths = storage::list_recent_reports_with_paths(conn, HOUSE_VIEW_RECENT_REPORTS)
        .unwrap_or_default();

    // The freshness gate dates the newest report's `created_at` to its **ET
    // session**, pairing with the ET `today` the caller passes. Both legs must
    // convert together: a report's stored `created_at` is a UTC instant, so an
    // afternoon-ET report keeps its own ET date under a prefix read while an
    // evening-ET run's `today` has already rolled — the two events straddle the
    // ~8 PM ET boundary and the gap reads one day long, retiring a 7-ET-day-old
    // view at the age limit. Converting only `today` would invert the error for
    // reports written after the boundary. An unparseable stamp (never
    // app-produced) fails soft to feeding the view.
    let stale = with_paths.first().is_some_and(|(s, _)| {
        crate::market_clock::et_date_of(&s.created_at)
            .map(|d| (today - d).num_days() > HOUSE_VIEW_MAX_AGE_DAYS)
            .unwrap_or(false)
    });
    if stale {
        return (
            HouseView {
                recent_summaries: Vec::new(),
                latest_sections: None,
            },
            true,
        );
    }

    let recent_summaries: Vec<ReportSummary> =
        with_paths.iter().map(|(s, _)| s.clone()).collect();

    // The latest report's body — the first (newest) entry's Markdown — read from disk
    // and reduced to the sections a holding's house view leans on.
    let latest_sections = with_paths
        .first()
        .and_then(|(_, path)| resolve_report_path(reports_dir, path))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .as_deref()
        .map(extract_house_view_sections)
        .filter(|s| !s.is_empty());

    (
        HouseView {
            recent_summaries,
            latest_sections,
        },
        false,
    )
}

/// Resolve a stored Markdown path, tolerating a relative stored path by joining it
/// under `reports_dir`. An absolute stored path is used as-is.
fn resolve_report_path(reports_dir: &Path, stored: &str) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(stored);
    if p.is_absolute() {
        Some(p.to_path_buf())
    } else {
        Some(reports_dir.join(p))
    }
}

/// Section titles the house view keeps from the report Markdown (matched
/// case-insensitively on the `##`/`###` header text).
const HOUSE_VIEW_SECTION_TITLES: &[&str] = &["thesis", "investment strategy", "forward outlook"];

/// Cap on the extracted house-view prose, so a long report can't dominate the prompt.
const HOUSE_VIEW_CHAR_CAP: usize = 6_000;

/// Pull the Thesis / Investment Strategy / Forward Outlook sections out of the report
/// Markdown by header, concatenating each matched section's body. A section runs from
/// its header to the next header of the same-or-higher level. Bounded by
/// [`HOUSE_VIEW_CHAR_CAP`] so the house view stays a context input, not the whole report.
pub fn extract_house_view_sections(markdown: &str) -> String {
    let mut out = String::new();
    // The level the currently-kept section's header sits at; `None` = not
    // capturing. A deeper header inside a kept section belongs to the section;
    // only a same-or-higher header re-decides — including a level-1 `#`, which
    // can never itself start a kept section but must end one.
    let mut capturing_level: Option<usize> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if capturing_level.is_some_and(|cap| level > cap) {
                out.push_str(trimmed);
                out.push('\n');
                if out.len() >= HOUSE_VIEW_CHAR_CAP {
                    break;
                }
                continue;
            }
            let title = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
            let keeps = (2..=3).contains(&level)
                && HOUSE_VIEW_SECTION_TITLES.iter().any(|t| title.contains(t));
            capturing_level = keeps.then_some(level);
            if keeps {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(trimmed);
                out.push('\n');
            }
            continue;
        }
        if capturing_level.is_some() {
            out.push_str(line);
            out.push('\n');
            if out.len() >= HOUSE_VIEW_CHAR_CAP {
                break;
            }
        }
    }
    // Whole-line pushes overshoot the cap, so the final cut must respect char
    // boundaries: `String::truncate` panics mid-character, and report prose is
    // full of multi-byte punctuation (em-dashes, curly quotes).
    if out.len() > HOUSE_VIEW_CHAR_CAP {
        let mut cut = HOUSE_VIEW_CHAR_CAP;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out.trim().to_string()
}

/// Look up the prior run's carry-over for one holding (the continuity input): the
/// verdict plus the audit-row legs — the grade-band parameter version its letter
/// was computed under (`None` = a pre-stamp run, i.e. the v1 bands) and the
/// pre-profit overlay record whose observation history accumulates. Reads the
/// latest persisted run and finds the matching symbol; `None` on a first run or a
/// newly-added holding. Fail-soft — a read error reads as "no prior verdict".
pub fn prior_verdict_for(conn: &Connection, symbol: &str) -> Option<PriorHolding> {
    let run = crate::portfolio::store::latest_run(conn).ok().flatten()?;
    let verdict = run
        .verdicts
        .into_iter()
        .find(|v| v.symbol.eq_ignore_ascii_case(symbol))?;
    // The verdict's effective vintage, not the container run's `created_at`: a
    // selective carry re-persists an old verdict (and its audit's authoring-spot
    // basis) into a newer run, so dating the retrospective off the container
    // would pair run A's spot with run B's date (Codex round 2, finding 2).
    let vintage = crate::portfolio::effective_vintage(&verdict, &run.created_at).to_string();
    let audit_row = run
        .audit
        .into_iter()
        .find(|a| a.symbol.eq_ignore_ascii_case(symbol));
    let (grade_parameter_version, pre_profit, spot) = match audit_row {
        Some(a) => {
            let spot = a.quick_basis.as_ref().map(|b| b.spot);
            (a.grade_parameter_version, a.pre_profit, spot)
        }
        None => (None, None, None),
    };
    // The prior run's matured outcome lines for this symbol — the deterministic
    // scored ground the retrospective block renders (empty until windows mature).
    let matured_notes = run
        .outcome
        .as_ref()
        .map(|o| {
            o.matured
                .iter()
                .filter(|m| m.symbol.eq_ignore_ascii_case(symbol))
                .map(|m| {
                    let detail = match (m.total_return, m.price_return) {
                        (Some(tr), _) => format!("total return {:+.1}%", tr * 100.0),
                        (None, Some(pr)) => format!("price-only return {:+.1}%", pr * 100.0),
                        _ => m.outcome.clone(),
                    };
                    format!("{}-month window {}: {}", m.window_months, m.outcome, detail)
                })
                .collect()
        })
        .unwrap_or_default();
    Some(PriorHolding {
        verdict,
        grade_parameter_version,
        pre_profit,
        vintage,
        spot,
        matured_notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::engine::{QuarterlyCashFlowRow, QuarterlyIncomeRow};
    use crate::portfolio::{AssetClass, PositionChange, VerdictDisposition};
    use crate::schwab::{Holdings, Position};

    /// A minimal persisted report for the house-view freshness tests.
    fn insert_house_view_report(conn: &Connection, id: &str, created_at: &str) {
        use crate::agent::{MarketCycle, RiskPosture, ThesisStance};
        let summary = ReportSummary {
            report_id: id.to_string(),
            report_type: "weekly_market".to_string(),
            created_at: created_at.to_string(),
            title: "Sample headline".to_string(),
            risk_posture: RiskPosture::Mixed,
            market_cycle: MarketCycle::LateCycle,
            thesis_stance: ThesisStance::Uncertain,
            header_summary_bullets: vec!["a".to_string()],
            key_risks: vec![],
            unresolved_questions: vec![],
            forward_outlook_themes: vec![],
        };
        let summary_json = serde_json::to_string(&summary).unwrap();
        storage::insert_report(
            conn,
            &storage::ReportRecord {
                summary: &summary,
                markdown_path: &format!("/nonexistent/{id}.md"),
                summary_json: &summary_json,
            },
        )
        .unwrap();
    }

    #[test]
    fn house_view_older_than_a_week_is_omitted_whole_and_flagged() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        insert_house_view_report(&conn, "r-old", "2026-07-20T14:00:00Z");
        // 8 ET days later — past the pinned window; the whole view drops (summaries
        // included), and the omission is reported for the data-health gap record.
        // The stamp is mid-session (10 AM ET) so the report's ET date is
        // unambiguously its own UTC date — a midnight-UTC stamp would belong to
        // the PRIOR ET session and test a different interval than it reads.
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 28).unwrap();
        let (view, omitted) = load_house_view(&conn, Path::new("/nonexistent"), today);
        assert!(omitted);
        assert!(view.recent_summaries.is_empty());
        assert!(view.latest_sections.is_none());
    }

    #[test]
    fn house_view_exactly_a_week_old_is_still_fed() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        insert_house_view_report(&conn, "r-week", "2026-07-21T14:00:00Z");
        // "Older than one week" is strict: exactly 7 ET days still feeds. Mid-session
        // stamp for the same reason as the sibling test above.
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 28).unwrap();
        let (view, omitted) = load_house_view(&conn, Path::new("/nonexistent"), today);
        assert!(!omitted);
        assert_eq!(view.recent_summaries.len(), 1);
    }

    #[test]
    fn an_evening_et_run_does_not_age_out_a_seven_et_day_old_house_view() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        // Written 3 PM ET on 2026-07-21 — still 2026-07-21 in UTC, so the prefix read
        // and the ET read agree on the report's side.
        insert_house_view_report(&conn, "r-week", "2026-07-21T19:00:00Z");
        // The run is 9 PM ET on 2026-07-28: seven ET days later, but its UTC instant
        // has already rolled to the 29th. Under the old UTC-prefix gate the two events
        // straddled the ~8 PM ET boundary and the gap read eight days, dropping the
        // whole view — summaries included — on every evening run at the age limit.
        let evening = chrono::DateTime::parse_from_rfc3339("2026-07-29T01:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let today = crate::market_clock::et_session_date(evening);
        assert_eq!(today, chrono::NaiveDate::from_ymd_opt(2026, 7, 28).unwrap());
        let (view, omitted) = load_house_view(&conn, Path::new("/nonexistent"), today);
        assert!(!omitted, "a 7-ET-day-old view must survive an evening-ET run");
        assert_eq!(view.recent_summaries.len(), 1);
    }

    #[test]
    fn a_report_written_after_the_et_rollover_ages_from_its_own_session() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        // 9 PM ET on 2026-07-21 — the UTC stamp says the 22nd, the ET session the 21st.
        // Converting only the run's `today` would have inverted the error here, reading
        // this report a day YOUNGER than it is; both legs convert, so it ages from the
        // 21st and is exactly 8 ET days stale.
        insert_house_view_report(&conn, "r-late", "2026-07-22T01:00:00Z");
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 29).unwrap();
        let (_, omitted) = load_house_view(&conn, Path::new("/nonexistent"), today);
        assert!(omitted);
    }

    #[test]
    fn house_view_with_no_reports_is_empty_but_not_flagged() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        // Nothing exists to omit — an empty store is a debut, not a staleness gap.
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 28).unwrap();
        let (view, omitted) = load_house_view(&conn, Path::new("/nonexistent"), today);
        assert!(!omitted);
        assert!(view.recent_summaries.is_empty());
    }

    fn fmp_only() -> CompanyFinancials {
        CompanyFinancials {
            symbol: "AAPL".into(),
            current_price: Some(195.0),
            market_cap: Some(3.0e12),
            shares_outstanding: Some(1.5e10),
            price_history: vec![180.0, 190.0, 195.0],
            ..CompanyFinancials::default()
        }
    }

    #[test]
    fn merge_fills_statement_lines_from_sec_and_derives_multiples() {
        let sec = CompanyFacts {
            revenue: Some(400_000_000_000),
            revenue_prior: Some(360_000_000_000),
            gross_profit: Some(180_000_000_000),
            net_income: Some(100_000_000_000),
            total_assets: Some(350_000_000_000),
            stockholders_equity: Some(60_000_000_000),
        };
        let merged = merge_financials(fmp_only(), &sec, false);
        // SEC filled the empty statement lines (annual basis — no TTM adoption).
        assert_eq!(merged.revenue, Some(400_000_000_000.0));
        assert_eq!(merged.revenue_prior, Some(360_000_000_000.0));
        assert_eq!(merged.net_income, Some(100_000_000_000.0));
        assert_eq!(merged.total_equity, Some(60_000_000_000.0));
        // Multiples derived from market cap (3e12): P/E=30, P/S=7.5, P/B=50.
        assert!((merged.pe_ratio.unwrap() - 30.0).abs() < 1e-6);
        assert!((merged.ps_ratio.unwrap() - 7.5).abs() < 1e-6);
        assert!((merged.pb_ratio.unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn merge_keeps_fmp_supplied_multiples_and_leaves_missing_inputs_as_gaps() {
        let mut fmp = fmp_only();
        fmp.pe_ratio = Some(28.0); // FMP already supplied a P/E
        let sec = CompanyFacts::default(); // SEC contributed nothing
        let merged = merge_financials(fmp, &sec, false);
        assert_eq!(merged.pe_ratio, Some(28.0), "FMP value not overwritten");
        // No revenue anywhere -> P/S stays a gap rather than fabricated.
        assert!(merged.ps_ratio.is_none());
        assert!(merged.revenue.is_none());
    }

    /// Quarterly rows for the TTM tests, newest-first: revenue 100/99/98/…, net
    /// income a fixed 22% of revenue, gross profit / cost of revenue set per test.
    fn quarters(n: usize, gross_profit: bool, cost_of_revenue: bool) -> Vec<QuarterlyIncomeRow> {
        let ends = [
            "2026-06-30", "2026-03-31", "2025-12-31", "2025-09-30",
            "2025-06-30", "2025-03-31", "2024-12-31", "2024-09-30",
        ];
        ends.iter()
            .take(n)
            .enumerate()
            .map(|(i, end)| {
                let revenue = 100.0 - i as f64;
                QuarterlyIncomeRow {
                    period_end: end.to_string(),
                    filing_date: None,
                    revenue: Some(revenue),
                    eps_diluted: None,
                    diluted_shares: None,
                    net_income: Some(0.22 * revenue),
                    operating_income: None,
                    gross_profit: gross_profit.then_some(0.45 * revenue),
                    cost_of_revenue: cost_of_revenue.then_some(0.55 * revenue),
                }
            })
            .collect()
    }

    #[test]
    fn a_zero_row_quarterly_response_with_a_sec_fallback_stamps_the_annual_basis() {
        use crate::portfolio::StatementBasis;
        // The hole an earlier shape of this stamp left open. FMP returns an EMPTY
        // quarterly set (the same empty-200 pattern the sector-P/E snapshot serves),
        // so no TTM window can be adopted and the SEC same-concept annual facts fill
        // the levels instead. Stamped `None` — "no statement basis applies" — those
        // annual levels slipped past the ledger's basis-continuity gate entirely,
        // because the gate only acts on a `Some` basis: a TTM-authored P/S threshold
        // would then be compared against an annual-basis ratio and could confirm, on
        // a market cadence, the fabricated crossing the gate exists to stop.
        //
        // The multiples are the reachable half: they key their observation on the
        // marks' trading day, not on a statement print, so they resolve normally even
        // with zero quarterly rows (the filing-cadence series go unevaluable for want
        // of a period end to key on).
        let mut fin = CompanyFinancials {
            symbol: "AAPL".into(),
            market_cap: Some(3_000_000_000_000.0),
            ..Default::default()
        };
        assert!(fin.quarterly_income.is_empty());
        assert!(!apply_ttm_statement_basis(&mut fin), "no window to adopt");
        assert_eq!(
            fin.statement_basis, None,
            "FMP alone supports no basis — the merge refines it"
        );
        let sec = CompanyFacts {
            revenue: Some(400_000_000_000),
            revenue_prior: Some(360_000_000_000),
            gross_profit: Some(180_000_000_000),
            net_income: Some(100_000_000_000),
            total_assets: Some(350_000_000_000),
            stockholders_equity: Some(60_000_000_000),
        };
        let merged = merge_financials(fin, &sec, false);
        assert_eq!(
            merged.statement_basis,
            Some(StatementBasis::Annual),
            "levels filled from annual facts stand on the annual basis"
        );
        assert!(merged.ps_ratio.is_some(), "the multiple the gate must cover resolves");
    }

    #[test]
    fn a_holding_with_no_statement_levels_at_all_carries_no_basis() {
        // The case `None` is actually for: nothing supplied a statement level, so
        // there is no basis to disagree with. A fund reaches the engine this way (it
        // skips the facts call), and `None` must keep meaning this rather than
        // doubling as "annual".
        let mut fin = CompanyFinancials {
            symbol: "ITOT".into(),
            market_cap: Some(1_000_000.0),
            ..Default::default()
        };
        assert!(!apply_ttm_statement_basis(&mut fin));
        let merged = merge_financials(fin, &CompanyFacts::default(), false);
        assert_eq!(merged.statement_basis, None);
    }

    #[test]
    fn ttm_basis_adopts_four_quarter_sums_and_the_prior_window() {
        let mut fin = fmp_only();
        fin.quarterly_income = quarters(8, true, false);
        assert!(apply_ttm_statement_basis(&mut fin));
        // Newest four quarters: 100+99+98+97; prior four: 96+95+94+93.
        assert_eq!(fin.revenue, Some(394.0));
        assert_eq!(fin.revenue_prior, Some(378.0));
        assert!((fin.net_income.unwrap() - 0.22 * 394.0).abs() < 1e-9);
        assert!((fin.gross_profit.unwrap() - 0.45 * 394.0).abs() < 1e-9);
    }

    #[test]
    fn ttm_basis_rejects_a_non_contiguous_four_quarter_window() {
        // A feed gap (a skipped quarter) makes the "four newest rows" span more
        // than twelve months — adoption must fail onto the annual-fallback path
        // rather than sum a 15-month "TTM" into every multiple.
        let mut fin = fmp_only();
        let mut rows = quarters(5, true, false);
        rows.remove(1); // drop 2026-03-31: newest four now span 2025-06-30..2026-06-30
        fin.quarterly_income = rows;
        assert!(!apply_ttm_statement_basis(&mut fin), "gapped window must not adopt");
        assert!(fin.revenue.is_none(), "no partial fill on the failed adoption");
    }

    #[test]
    fn ttm_prior_window_gaps_on_a_non_contiguous_eight_quarter_run() {
        // The TTM window itself is contiguous, but a gap inside the prior four
        // shifts the YoY comparison a quarter — growth gaps rather than
        // misaligning, while the TTM basis itself stands.
        let mut fin = fmp_only();
        let mut rows = quarters(8, true, false);
        rows.remove(5); // drop 2025-03-31 from the prior window
        fin.quarterly_income = rows;
        assert!(apply_ttm_statement_basis(&mut fin), "the TTM four are intact");
        assert_eq!(fin.revenue, Some(394.0));
        assert_eq!(fin.revenue_prior, None, "misaligned prior window must gap");
    }

    #[test]
    fn loss_maker_pe_derives_negative_so_the_engine_guard_is_reachable() {
        // The signed P/E derive (grade-v2.1): a loss-maker must produce a
        // NEGATIVE P/E — `None` silently excused every loss-maker from the
        // engine's fixed "a loss-maker is never cheap" valuation score, a
        // ~25-point axis escape. P/B keeps the positive-denominator derive.
        let mut fin = fmp_only();
        fin.market_cap = Some(1_000.0);
        fin.net_income = Some(-50.0);
        fin.total_equity = Some(-10.0);
        let merged = merge_financials(fin, &CompanyFacts::default(), true);
        assert_eq!(merged.pe_ratio, Some(-20.0));
        assert_eq!(merged.pb_ratio, None, "negative equity has no P/B read");
        let metrics = crate::portfolio::engine::compute_metrics(&merged);
        assert_eq!(metrics.pe_ratio, Some(-20.0));
    }

    #[test]
    fn ttm_gross_profit_derives_from_cost_of_revenue_when_unreported() {
        let mut fin = fmp_only();
        fin.quarterly_income = quarters(4, false, true);
        assert!(apply_ttm_statement_basis(&mut fin));
        // Σ(rev − cor) over the newest four: 0.45 × 394.
        assert!((fin.gross_profit.unwrap() - 0.45 * 394.0).abs() < 1e-9);
        // Only four quarters — the prior window stays an honest gap.
        assert_eq!(fin.revenue_prior, None);
    }

    #[test]
    fn ttm_gross_profit_mixes_reported_and_derived_quarters() {
        let mut fin = fmp_only();
        let mut rows = quarters(4, true, true);
        // Two quarters report the gross line, two derive theirs from cost of
        // revenue — the sum must not gap on the mix.
        rows[1].gross_profit = None;
        rows[3].gross_profit = None;
        fin.quarterly_income = rows;
        assert!(apply_ttm_statement_basis(&mut fin));
        assert!((fin.gross_profit.unwrap() - 0.45 * 394.0).abs() < 1e-9);
    }

    #[test]
    fn thin_quarters_fall_back_to_the_sec_annual_basis() {
        let mut fin = fmp_only();
        fin.quarterly_income = quarters(3, true, false);
        assert!(!apply_ttm_statement_basis(&mut fin), "three quarters cannot sum a TTM");
        assert!(fin.revenue.is_none(), "no partial fill on the failed adoption");
        let sec = CompanyFacts {
            revenue: Some(400),
            revenue_prior: Some(360),
            net_income: Some(88),
            ..CompanyFacts::default()
        };
        let merged = merge_financials(fin, &sec, false);
        assert_eq!(merged.revenue, Some(400.0));
        assert_eq!(merged.revenue_prior, Some(360.0), "annual growth pair from SEC");
    }

    #[test]
    fn ttm_basis_never_mixes_an_annual_gross_profit_into_the_margin() {
        let mut fin = fmp_only();
        // TTM adopts on revenue + net income, but no quarter carries a gross line.
        fin.quarterly_income = quarters(4, false, false);
        assert!(apply_ttm_statement_basis(&mut fin));
        assert!(fin.gross_profit.is_none());
        let sec = CompanyFacts {
            gross_profit: Some(180), // an annual print over a TTM denominator would fabricate a margin
            stockholders_equity: Some(60),
            ..CompanyFacts::default()
        };
        let merged = merge_financials(fin, &sec, true);
        assert!(merged.gross_profit.is_none(), "the annual line must not ride the TTM basis");
        assert_eq!(merged.total_equity, Some(60.0), "the balance-sheet instant still fills");
    }

    #[test]
    fn extract_pulls_only_the_house_view_sections() {
        let md = "\
# Market Signal Report

## Header Summary
- a bullet

## Market Signal Thesis
Rotation, not rupture. Breadth is the tell.

## Index Picture
Dow up, Nasdaq down.

## Investment Strategy
Stay long quality; fade the speculative tail.

## Forward Outlook
Watch the 2s10s and the labor prints.
";
        let sections = extract_house_view_sections(md);
        assert!(sections.contains("Rotation, not rupture"), "{sections}");
        assert!(sections.contains("Stay long quality"), "{sections}");
        assert!(sections.contains("Watch the 2s10s"), "{sections}");
        // Non-house-view sections are excluded.
        assert!(!sections.contains("Dow up"), "{sections}");
        assert!(!sections.contains("a bullet"), "{sections}");
    }

    #[test]
    fn extract_honors_the_section_level_contract() {
        // A `###` sub-heading inside a kept `##` section belongs to it (the
        // pre-fix walk ended capture there, silently dropping the section's
        // remainder); a level-1 `#` heading ends capture (it previously failed
        // the "##" strip and leaked unrelated prose in).
        let md = "\
## Forward Outlook
Watch the 2s10s.

### Rates
The long end is the swing factor.

# Appendix
Sources and footnotes.
";
        let sections = extract_house_view_sections(md);
        assert!(sections.contains("Watch the 2s10s"), "{sections}");
        assert!(
            sections.contains("The long end is the swing factor"),
            "a ### subsection inside a kept section was dropped: {sections}"
        );
        assert!(
            !sections.contains("Sources and footnotes"),
            "a level-1 heading must end capture: {sections}"
        );
    }

    #[test]
    fn extract_truncates_multi_byte_prose_without_panicking() {
        // Whole-line pushes overshoot the byte cap, and report prose is full of
        // multi-byte punctuation — the final cut must respect char boundaries
        // (`String::truncate` panics mid-character; the panic unwound past every
        // run_finished emitter, stranding the tracker with no Failed row).
        // Header (24 bytes) + "x" puts the em-dash run at byte 25; the cap at
        // 6000 lands (6000 − 25) = 5975 bytes in — 5975 mod 3 = 2, mid-dash by
        // construction, so the pre-fix truncate reliably panicked here.
        let md = format!("## Market Signal Thesis\nx{}\n", "—".repeat(9_000));
        let sections = extract_house_view_sections(&md);
        assert!(sections.len() <= HOUSE_VIEW_CHAR_CAP, "{}", sections.len());
        assert!(sections.starts_with("## Market Signal Thesis"), "{sections}");
    }

    #[test]
    fn assemble_records_sources_and_computes_the_options_signal() {
        use crate::schwab::{OptionKind, OptionQuote};
        let position = Position {
            symbol: "AAPL".into(),
            description: "Apple".into(),
            asset_class: AssetClass::Stock,
            quantity: 100.0,
            cost_basis: 14_000.0,
            market_value: 19_500.0,
            current_price: Some(195.0),
        };
        let chain = OptionChain {
            underlying: "AAPL".into(),
            underlying_price: Some(195.0),
            contracts: vec![
                OptionQuote {
                    kind: OptionKind::Call,
                    strike: 195.0,
                    expiry: "2026-07-17".into(),
                    volume: 1000.0,
                    open_interest: 5000.0,
                    implied_volatility: Some(0.25),
                },
                OptionQuote {
                    kind: OptionKind::Put,
                    strike: 195.0,
                    expiry: "2026-07-17".into(),
                    volume: 1500.0,
                    open_interest: 6000.0,
                    implied_volatility: Some(0.31),
                },
            ],
        };
        let sec = CompanyFacts {
            revenue: Some(400_000_000_000),
            ..CompanyFacts::default()
        };
        let dossier = assemble(
            position,
            PositionDelta::new_position(),
            fmp_only(),
            &sec,
            Some(&chain),
            InvestorProfile::default_fixture(),
            HouseView::default(),
            None,
            None,
            None,
        );
        assert!(dossier.sources.iter().any(|s| s.contains("FMP")));
        assert!(dossier.sources.iter().any(|s| s.contains("SEC")));
        assert!(dossier.sources.iter().any(|s| s.contains("option chain")));
        assert!(dossier.options_signal.put_call_volume.unwrap() > 1.0);
        assert!(dossier.prior_verdict.is_none(), "new holding");
    }

    #[test]
    fn ttm_basis_resolves_conflicting_duplicate_periods_to_the_latest_filing() {
        // The newest quarter served twice with different prints (a restatement):
        // the later-filed row must set the TTM basis in either arrival order —
        // the letter cannot depend on response ordering.
        let base = |order_restated_first: bool| {
            let mut rows = quarters(8, true, false);
            rows[0].filing_date = Some("2026-07-01".into());
            let mut restated = rows[0].clone();
            restated.revenue = Some(150.0);
            restated.filing_date = Some("2026-08-01".into());
            if order_restated_first {
                rows.insert(0, restated);
            } else {
                rows.push(restated);
            }
            let mut fin = CompanyFinancials {
                quarterly_income: rows,
                ..CompanyFinancials::default()
            };
            assert!(apply_ttm_statement_basis(&mut fin));
            fin.revenue
        };
        let tail = base(false);
        let head = base(true);
        assert_eq!(tail, head, "order-independent");
        // Restated 150 + the next three quarters (99 + 98 + 97).
        assert_eq!(tail, Some(150.0 + 99.0 + 98.0 + 97.0));
    }

    #[test]
    fn ttm_basis_canonicalizes_the_statement_rows_in_place() {
        // An out-of-order feed (oldest first): the basis must read the true newest
        // four — not the wire head — AND leave both statement vecs canonical on the
        // fin, so every later reader (the driver ladder's trailing prints and share
        // basis, the anchor windows, the pre-profit leg) inherits the same order.
        let mut fin = fmp_only();
        let mut rows = quarters(8, true, false);
        rows.reverse(); // wire head becomes 2024-09-30
        fin.quarterly_income = rows;
        fin.quarterly_cash_flow = vec![
            QuarterlyCashFlowRow {
                period_end: "2026-03-31".into(),
                filing_date: None,
                free_cash_flow: Some(2.0),
                operating_cash_flow: None,
                capex: None,
            },
            QuarterlyCashFlowRow {
                period_end: "2026-06-30".into(),
                filing_date: None,
                free_cash_flow: Some(3.0),
                operating_cash_flow: None,
                capex: None,
            },
        ];
        assert!(apply_ttm_statement_basis(&mut fin));
        // The sums read the true newest four quarters, not the reversed head.
        assert_eq!(fin.revenue, Some(394.0));
        assert_eq!(fin.revenue_prior, Some(378.0));
        // And the rows are now canonical in place, on both statements.
        assert_eq!(fin.quarterly_income[0].period_end, "2026-06-30");
        assert_eq!(fin.quarterly_income[7].period_end, "2024-09-30");
        assert_eq!(fin.quarterly_cash_flow[0].period_end, "2026-06-30");
    }

    #[test]
    fn statement_rows_canonicalize_even_when_the_basis_is_not_adopted() {
        // Three quarters cannot sum a TTM, but the rows still canonicalize: the
        // driver ladder and anchor windows read this fin next, and their order
        // guarantee must not depend on whether the basis adopted.
        let mut fin = fmp_only();
        let mut rows = quarters(3, true, false);
        rows.reverse();
        fin.quarterly_income = rows;
        assert!(!apply_ttm_statement_basis(&mut fin));
        assert_eq!(fin.quarterly_income[0].period_end, "2026-06-30");
        assert_eq!(fin.quarterly_income[2].period_end, "2025-12-31");
    }

    #[test]
    fn prior_verdict_lookup_reads_the_latest_run() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        // No runs yet -> no prior verdict.
        assert!(prior_verdict_for(&conn, "AAPL").is_none());

        // Persist a run carrying an AAPL verdict; the lookup finds it.
        let run = crate::portfolio::PortfolioRun {
            run_id: "r1".into(),
            created_at: "2026-06-20T00:00:00Z".into(),
            holdings: Holdings {
                positions: vec![],
                cash: 0.0,
                account_total: 0.0,
                source_rows: vec![],
            },
            verdicts: vec![HoldingVerdict {
                symbol: "AAPL".into(),
                asset_class: AssetClass::Stock,
                position_change: PositionChange::New,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                thesis_ledger: None,
                analyzed_at: None,
                action_source: Default::default(),
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                aggregates: None,
                construction: None,
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: None,
                overview: String::new(),
            },
            audit: vec![],
            rate_prints: None,
            outcome: None,
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let prior = prior_verdict_for(&conn, "aapl").expect("case-insensitive match");
        assert_eq!(prior.verdict.symbol, "AAPL");
        // No audit row for the symbol -> a pre-stamp read (the v1 bands).
        assert_eq!(prior.grade_parameter_version, None);
        assert!(prior.pre_profit.is_none());
    }

    #[test]
    fn prior_verdict_lookup_carries_the_stamped_grade_parameter_version() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        let run = crate::portfolio::PortfolioRun {
            run_id: "r1".into(),
            created_at: "2026-08-03T00:00:00Z".into(),
            holdings: Holdings {
                positions: vec![],
                cash: 0.0,
                account_total: 0.0,
                source_rows: vec![],
            },
            verdicts: vec![HoldingVerdict {
                symbol: "AAPL".into(),
                asset_class: AssetClass::Stock,
                position_change: PositionChange::New,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                thesis_ledger: None,
                analyzed_at: None,
                action_source: Default::default(),
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                aggregates: None,
                construction: None,
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: None,
                overview: String::new(),
            },
            audit: vec![crate::portfolio::HoldingAudit {
                symbol: "AAPL".into(),
                metrics: Default::default(),
                sources: vec![],
                model_ids: vec![],
                prompt_version: crate::portfolio::PROMPT_VERSION.to_string(),
                degraded_inputs: vec![],
                target_meta: None,
                grade_parameter_version: Some("grade-v2".into()),
                ledger_audit: None,
                quick_basis: None,
                fund_exposure: None,
                pre_profit: None,
                hurdle: None,
            }],
            rate_prints: None,
            outcome: None,
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let prior = prior_verdict_for(&conn, "AAPL").expect("verdict present");
        assert_eq!(prior.grade_parameter_version.as_deref(), Some("grade-v2"));
        // No `analyzed_at` on the verdict -> the vintage falls back to the
        // container run's `created_at`.
        assert_eq!(prior.vintage, "2026-08-03T00:00:00Z");
    }

    #[test]
    fn prior_verdict_lookup_dates_a_selective_carry_by_its_own_vintage() {
        // A selective run re-persists a carried verdict (its `analyzed_at` and
        // its audit's authoring-spot basis intact) into a newer container run.
        // The retrospective's "since" anchor must be the verdict's effective
        // vintage — the container run's `created_at` would pair run A's spot
        // with run B's date (Codex round 2, finding 2).
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        let run = crate::portfolio::PortfolioRun {
            run_id: "r2".into(),
            created_at: "2026-08-05T00:00:00Z".into(),
            holdings: Holdings {
                positions: vec![],
                cash: 0.0,
                account_total: 0.0,
                source_rows: vec![],
            },
            verdicts: vec![HoldingVerdict {
                symbol: "AAPL".into(),
                asset_class: AssetClass::Stock,
                position_change: PositionChange::Unchanged,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                thesis_ledger: None,
                analyzed_at: Some("2026-07-29T12:00:00Z".into()),
                action_source: Default::default(),
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                aggregates: None,
                construction: None,
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: None,
                overview: String::new(),
            },
            audit: vec![],
            rate_prints: None,
            outcome: None,
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let prior = prior_verdict_for(&conn, "AAPL").expect("verdict present");
        assert_eq!(prior.vintage, "2026-07-29T12:00:00Z");
    }
}
