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
    // Newest-first with the latest filing winning a duplicated period — the shared
    // canonicalization policy (`pre_profit::statement_inputs` holds the same rule):
    // a restatement served twice must resolve to the restated print, never to wire
    // order, or the TTM basis (and with it the letter) would depend on response
    // ordering.
    let mut rows: Vec<&crate::portfolio::engine::QuarterlyIncomeRow> =
        fin.quarterly_income.iter().collect();
    rows.sort_by(|a, b| {
        b.period_end
            .cmp(&a.period_end)
            .then_with(|| b.filing_date.cmp(&a.filing_date))
    });
    rows.dedup_by(|a, b| a.period_end == b.period_end);

    fn sum4(
        rows: &[&crate::portfolio::engine::QuarterlyIncomeRow],
        get: impl Fn(&crate::portfolio::engine::QuarterlyIncomeRow) -> Option<f64>,
    ) -> Option<f64> {
        if rows.len() < 4 {
            return None;
        }
        rows[..4].iter().map(|r| get(r)).sum()
    }
    let ttm_revenue = sum4(&rows, |r| r.revenue);
    let ttm_net_income = sum4(&rows, |r| r.net_income);
    let (Some(_), Some(_)) = (ttm_revenue, ttm_net_income) else {
        return false;
    };
    // Per-quarter: each quarter contributes its reported gross line, or derives
    // its own from revenue − cost of revenue — mixing reported and derived
    // quarters is fine (same economic line per quarter), a quarter with neither
    // gaps the whole sum.
    let ttm_gross_profit = sum4(&rows, |r| {
        r.gross_profit
            .or_else(|| Some(r.revenue? - r.cost_of_revenue?))
    });
    let prior = if rows.len() >= 8 { &rows[4..8] } else { &[][..] };
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

    // Derive multiples from market cap + fundamentals when FMP didn't supply them.
    let derive = |num: Option<f64>, den: Option<f64>| match (num, den) {
        (Some(n), Some(d)) if d > 0.0 => Some(n / d),
        _ => None,
    };
    if fmp.pe_ratio.is_none() {
        fmp.pe_ratio = derive(fmp.market_cap, fmp.net_income);
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
) -> HoldingDossier {
    let (prior_verdict, prior_grade_parameter_version, prior_pre_profit) = match prior {
        Some(p) => (Some(p.verdict), p.grade_parameter_version, p.pre_profit),
        None => (None, None, None),
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

    let mut sources = vec!["FMP company financials".to_string()];
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
        prior_grade_parameter_version,
        prior_pre_profit,
        sources,
    }
}

/// Load the Market Signal house view deterministically: the most recent
/// [`HOUSE_VIEW_RECENT_REPORTS`] report summaries and the latest report's relevant
/// prose sections. Fail-soft — an unreadable DB or missing Markdown degrades to a
/// thinner house view, never an error (the holding still grades on its fundamentals).
pub fn load_house_view(conn: &Connection, reports_dir: &Path) -> HouseView {
    let with_paths = storage::list_recent_reports_with_paths(conn, HOUSE_VIEW_RECENT_REPORTS)
        .unwrap_or_default();
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

    HouseView {
        recent_summaries,
        latest_sections,
    }
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
    let mut capturing = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("##") {
            // A header line (## or ###): decide whether this section is one we keep.
            let title = rest.trim_start_matches('#').trim().to_ascii_lowercase();
            capturing = HOUSE_VIEW_SECTION_TITLES
                .iter()
                .any(|t| title.contains(t));
            if capturing {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(line.trim_start());
                out.push('\n');
            }
            continue;
        }
        if capturing {
            out.push_str(line);
            out.push('\n');
            if out.len() >= HOUSE_VIEW_CHAR_CAP {
                break;
            }
        }
    }
    out.truncate(HOUSE_VIEW_CHAR_CAP);
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
    let audit_row = run
        .audit
        .into_iter()
        .find(|a| a.symbol.eq_ignore_ascii_case(symbol));
    let (grade_parameter_version, pre_profit) = match audit_row {
        Some(a) => (a.grade_parameter_version, a.pre_profit),
        None => (None, None),
    };
    Some(PriorHolding {
        verdict,
        grade_parameter_version,
        pre_profit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::engine::QuarterlyIncomeRow;
    use crate::portfolio::{AssetClass, PositionChange, VerdictDisposition};
    use crate::schwab::{Holdings, Position};

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
            }],
            rate_prints: None,
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let prior = prior_verdict_for(&conn, "AAPL").expect("verdict present");
        assert_eq!(prior.grade_parameter_version.as_deref(), Some("grade-v2"));
    }
}
