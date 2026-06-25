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
use crate::portfolio::{HoldingVerdict, InvestorProfile, OptionsSignal, HOUSE_VIEW_RECENT_REPORTS};
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
    pub financials: CompanyFinancials,
    pub options_signal: OptionsSignal,
    pub profile: InvestorProfile,
    pub house_view: HouseView,
    /// The prior run's verdict for this holding (continuity input), or `None` on a
    /// holding the job has not seen before ("new holding").
    pub prior_verdict: Option<HoldingVerdict>,
    /// The data sources that contributed, for the run's audit record.
    pub sources: Vec<String>,
}

/// Merge the keyless SEC EDGAR facts into the FMP per-company financials and derive
/// the valuation multiples from market cap plus statement lines. SEC fills the
/// statement fields FMP left empty (revenue, gross profit, net income, equity) — a
/// missing field stays a gap rather than a fabricated level — and the multiples are
/// computed only when both market cap and the denominator are present.
pub fn merge_financials(mut fmp: CompanyFinancials, sec: &CompanyFacts) -> CompanyFinancials {
    let fill = |dst: &mut Option<f64>, src: Option<i64>| {
        if dst.is_none() {
            if let Some(v) = src {
                *dst = Some(v as f64);
            }
        }
    };
    fill(&mut fmp.revenue, sec.revenue);
    fill(&mut fmp.gross_profit, sec.gross_profit);
    fill(&mut fmp.net_income, sec.net_income);
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
    fmp_financials: CompanyFinancials,
    sec_facts: &CompanyFacts,
    chain: Option<&OptionChain>,
    profile: InvestorProfile,
    house_view: HouseView,
    prior_verdict: Option<HoldingVerdict>,
) -> HoldingDossier {
    let financials = merge_financials(fmp_financials, sec_facts);
    let options_signal = chain
        .map(crate::portfolio::engine::options_signal)
        .unwrap_or(OptionsSignal {
            put_call_volume: None,
            put_call_open_interest: None,
            implied_volatility: None,
            iv_skew: None,
        });

    let mut sources = vec!["FMP company financials".to_string()];
    if !sec_facts.is_empty() {
        sources.push("SEC EDGAR company facts".to_string());
    }
    if chain.is_some() {
        sources.push("Schwab option chain".to_string());
    }
    if !house_view.recent_summaries.is_empty() || house_view.latest_sections.is_some() {
        sources.push("Market Signal Report (house view)".to_string());
    }

    HoldingDossier {
        position,
        financials,
        options_signal,
        profile,
        house_view,
        prior_verdict,
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

/// Look up the prior run's verdict for one holding (the continuity input). Reads the
/// latest persisted run and finds the matching symbol; `None` on a first run or a
/// newly-added holding. Fail-soft — a read error reads as "no prior verdict".
pub fn prior_verdict_for(conn: &Connection, symbol: &str) -> Option<HoldingVerdict> {
    let run = crate::portfolio::store::latest_run(conn).ok().flatten()?;
    run.verdicts
        .into_iter()
        .find(|v| v.symbol.eq_ignore_ascii_case(symbol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{AssetClass, VerdictDisposition};
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
            gross_profit: Some(180_000_000_000),
            net_income: Some(100_000_000_000),
            total_assets: Some(350_000_000_000),
            stockholders_equity: Some(60_000_000_000),
        };
        let merged = merge_financials(fmp_only(), &sec);
        // SEC filled the empty statement lines.
        assert_eq!(merged.revenue, Some(400_000_000_000.0));
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
        let merged = merge_financials(fmp, &sec);
        assert_eq!(merged.pe_ratio, Some(28.0), "FMP value not overwritten");
        // No revenue anywhere -> P/S stays a gap rather than fabricated.
        assert!(merged.ps_ratio.is_none());
        assert!(merged.revenue.is_none());
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
            fmp_only(),
            &sec,
            Some(&chain),
            InvestorProfile::default_fixture(),
            HouseView::default(),
            None,
        );
        assert!(dossier.sources.iter().any(|s| s.contains("FMP")));
        assert!(dossier.sources.iter().any(|s| s.contains("SEC")));
        assert!(dossier.sources.iter().any(|s| s.contains("option chain")));
        assert!(dossier.options_signal.put_call_volume.unwrap() > 1.0);
        assert!(dossier.prior_verdict.is_none(), "new holding");
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
            },
            verdicts: vec![HoldingVerdict {
                symbol: "AAPL".into(),
                asset_class: AssetClass::Stock,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                overview: String::new(),
            },
            audit: vec![],
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let prior = prior_verdict_for(&conn, "aapl").expect("case-insensitive match");
        assert_eq!(prior.symbol, "AAPL");
    }
}
