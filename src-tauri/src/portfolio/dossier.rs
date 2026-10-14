//! Deterministic dossier assembly (`docs/portfolio-analysis.md` §The per-holding
//! pipeline, step 1; `docs/local-models.md §Context-memory discipline`): the
//! financials merge (FMP plus SEC EDGAR, multiples derived from market cap and
//! statement lines), the house view loaded from the latest report's sections and
//! recent summaries, and the prior run's carry-over for the continuity check.

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

/// The audit's source label for the house view. Appended by the interpretation
/// paths in `pipeline`, never by [`assemble`] (see the note at its end).
pub const HOUSE_VIEW_SOURCE: &str = "Market Signal Report (house view)";

/// The Market Signal house view, loaded deterministically
/// (`docs/portfolio-analysis.md`) — never via the report's vector memory
/// (`docs/storage.md §Local Vector Memory`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HouseView {
    pub recent_summaries: Vec<ReportSummary>,
    /// The latest report's Thesis / Investment Strategy / Forward Outlook prose,
    /// concatenated; `None` when no report exists or none could be read.
    pub latest_sections: Option<String>,
}

/// Days each run-level commodity window covers: wide enough for ~13 monthly IMF
/// prints and a ~1-year daily trend base in one date-ranged request per series.
pub const COMMODITY_WINDOW_DAYS: i64 = 400;

/// Which commodity sleeve a print belongs to (`docs/data-sources.md §Portfolio
/// Analysis — endpoint surface`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CommodityGroup {
    Energy,
    Metals,
    Uranium,
    Gold,
}

/// One run-level commodity price read on the series' own published level basis
/// (never the rate normalization). The observation dates ride along because a
/// monthly IMF series lags by design and the model must read the print as-of.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommodityPrint {
    /// Display label ("WTI Crude Oil").
    pub label: String,
    /// The published unit ("USD per barrel").
    pub unit: String,
    pub group: CommodityGroup,
    pub latest: crate::portfolio::engine::DatedValue,
    /// The window's earliest print — the trailing trend base; `None` when the
    /// window returned a single print.
    pub trailing: Option<crate::portfolio::engine::DatedValue>,
}

/// The run-level commodity context, fetched once per run and shared across every
/// holding (`docs/portfolio-workflow.md` §Step 5). Empty prints and empty gaps
/// together mean the leg never ran.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CommodityContext {
    pub prints: Vec<CommodityPrint>,
    pub gaps: Vec<String>,
}

/// Select the prints that ride one holding's dossier by its FMP profile identity.
/// Industry overrides the broad sector where the sector misleads: uranium takes
/// its own sleeve, coal takes no oil / gas proxy, and gold is industry-only,
/// never the whole Basic Materials sector. Any other identity carries no
/// commodity block; the context is linked evidence, not a universal macro feed.
pub fn commodity_prints_for_holding(
    ctx: &CommodityContext,
    sector: Option<&str>,
    industry: Option<&str>,
) -> Vec<CommodityPrint> {
    let mut groups: Vec<CommodityGroup> = Vec::new();
    let industry = industry.map(str::to_ascii_lowercase);
    let uranium = industry.as_deref().is_some_and(|i| i.contains("uranium"));
    let coal = industry.as_deref().is_some_and(|i| i.contains("coal"));
    if uranium {
        groups.push(CommodityGroup::Uranium);
    } else if !coal {
        match sector {
            Some("Energy") => groups.push(CommodityGroup::Energy),
            Some("Basic Materials") => groups.push(CommodityGroup::Metals),
            _ => {}
        }
    }
    if industry.as_deref().is_some_and(|i| {
        i.contains("gold") || i.contains("precious metals")
    }) {
        groups.push(CommodityGroup::Gold);
    }
    if groups.is_empty() {
        return Vec::new();
    }
    ctx.prints
        .iter()
        .filter(|p| groups.contains(&p.group))
        .cloned()
        .collect()
}

// ---- Same-underlying option overlay (`docs/portfolio-workflow.md` §Step 6a) ----

/// A leg's side, read off the netted signed quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayDirection {
    Long,
    Short,
}

/// The overlay's deterministic classification (`docs/portfolio-analysis.md`
/// §The per-holding pipeline Step 1): a naked short call must never read as
/// covered, and an unrecognized multi-leg reads `other` with its net delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayClass {
    CoveredCall,
    ProtectivePut,
    Collar,
    Other,
}

/// One same-underlying option position, decoded and typed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OverlayLeg {
    /// The contract symbol as held (trimmed OCC form).
    pub contract: String,
    pub direction: OverlayDirection,
    /// Contracts held (absolute count; the side rides `direction`).
    pub quantity: f64,
    /// Call / put — `None` when the OCC symbol did not decode (the leg then
    /// forces the `other` classification).
    pub kind: Option<crate::schwab::OptionKind>,
    pub strike: Option<f64>,
    pub expiry: Option<String>,
    /// The contract's delta off the targeted chain fetch; `None` is the typed
    /// gap (a failed fetch or the sentinel).
    pub delta: Option<f64>,
}

/// The holding's typed same-underlying option overlay
/// (`docs/portfolio-workflow.md` §Step 6a): evidence for the verdict and the
/// action call, never a grade input.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OptionOverlay {
    pub legs: Vec<OverlayLeg>,
    pub class: OverlayClass,
    /// The classified strategy's covered fraction of the held shares —
    /// `(contracts × 100) ÷ shares` for the covering side; `None` on `other`.
    pub coverage_ratio: Option<f64>,
    /// Net delta in share-equivalents (Σ ±quantity × 100 × delta) — `None`
    /// whenever any leg's delta is a gap, never a partial sum.
    pub net_delta: Option<f64>,
    /// Whether the targeted delta fetch actually served or failed (vs never
    /// running — the stub default), for the audit's source labels.
    pub delta_source_consulted: bool,
    /// Typed gaps: unrecognized symbols, missing deltas, the naked-short-call
    /// note.
    pub gaps: Vec<String>,
}

/// `shares` is the holding's netted quantity; `delta_for` resolves a decoded
/// contract to its chain delta. `None` when no option rows exist.
pub fn assemble_option_overlay(
    shares: f64,
    option_rows: &[&Position],
    delta_for: impl Fn(&crate::schwab::OccContract) -> Option<f64>,
    delta_source_consulted: bool,
) -> Option<OptionOverlay> {
    let mut legs = Vec::new();
    let mut gaps = Vec::new();
    let mut unrecognized = false;
    // Recognized contract totals by (side, kind), in contracts.
    let (mut short_calls, mut long_calls, mut short_puts, mut long_puts) = (0.0, 0.0, 0.0, 0.0);
    for row in option_rows {
        // Defensive: the job's collector already excludes zero-net rows.
        if row.quantity == 0.0 {
            continue;
        }
        let direction = if row.quantity >= 0.0 {
            OverlayDirection::Long
        } else {
            OverlayDirection::Short
        };
        let quantity = row.quantity.abs();
        match crate::schwab::parse_occ_symbol(&row.symbol) {
            Some(c) => {
                use crate::schwab::OptionKind;
                match (direction, c.kind) {
                    (OverlayDirection::Short, OptionKind::Call) => short_calls += quantity,
                    (OverlayDirection::Long, OptionKind::Call) => long_calls += quantity,
                    (OverlayDirection::Short, OptionKind::Put) => short_puts += quantity,
                    (OverlayDirection::Long, OptionKind::Put) => long_puts += quantity,
                }
                let delta = delta_for(&c);
                if delta.is_none() {
                    gaps.push(format!("delta unavailable for {}", row.symbol.trim()));
                }
                legs.push(OverlayLeg {
                    contract: row.symbol.trim().to_string(),
                    direction,
                    quantity,
                    kind: Some(c.kind),
                    strike: Some(c.strike),
                    expiry: Some(c.expiry),
                    delta,
                });
            }
            None => {
                unrecognized = true;
                gaps.push(format!(
                    "unrecognized contract symbol {} — classified other",
                    row.symbol.trim()
                ));
                legs.push(OverlayLeg {
                    contract: row.symbol.trim().to_string(),
                    direction,
                    quantity,
                    kind: None,
                    strike: None,
                    expiry: None,
                    delta: None,
                });
            }
        }
    }

    if legs.is_empty() {
        return None;
    }

    // Single-strategy shapes only; anything else is `other`. A short call is
    // covered only up to the held shares; any excess is naked and must never
    // read as covered.
    let covered = |contracts: f64| contracts * 100.0 <= shares + 1e-9;
    let naked_short_call = short_calls > 0.0 && (shares <= 0.0 || !covered(short_calls));
    if naked_short_call {
        gaps.push(
            "short calls exceed the held shares (naked) — never reads covered".to_string(),
        );
    }
    let class = if unrecognized || shares <= 0.0 || naked_short_call {
        OverlayClass::Other
    } else if short_calls > 0.0 && long_puts > 0.0 && long_calls == 0.0 && short_puts == 0.0 {
        OverlayClass::Collar
    } else if short_calls > 0.0 && long_puts == 0.0 && long_calls == 0.0 && short_puts == 0.0 {
        OverlayClass::CoveredCall
    } else if long_puts > 0.0 && short_calls == 0.0 && long_calls == 0.0 && short_puts == 0.0 {
        OverlayClass::ProtectivePut
    } else {
        OverlayClass::Other
    };
    let coverage_ratio = match class {
        OverlayClass::CoveredCall => Some(short_calls * 100.0 / shares),
        OverlayClass::ProtectivePut => Some(long_puts * 100.0 / shares),
        // A collar's covered fraction is its narrower side.
        OverlayClass::Collar => Some(short_calls.min(long_puts) * 100.0 / shares),
        OverlayClass::Other => None,
    };
    // Net delta in share-equivalents — whole or not at all: a partial sum over
    // gapped legs would fabricate a hedged read.
    let net_delta = legs
        .iter()
        .map(|l| {
            l.delta.map(|d| {
                let sign = match l.direction {
                    OverlayDirection::Long => 1.0,
                    OverlayDirection::Short => -1.0,
                };
                sign * l.quantity * 100.0 * d
            })
        })
        .sum::<Option<f64>>();
    Some(OptionOverlay {
        legs,
        class,
        coverage_ratio,
        net_delta,
        delta_source_consulted,
        gaps,
    })
}

/// One sector benchmark's dated closes (`docs/data-sources.md §Financial
/// Modeling Prep`), fetched run-level and memoized per symbol.
#[derive(Debug, Clone)]
pub struct BenchmarkSeries {
    pub symbol: String,
    pub closes: Vec<crate::portfolio::engine::DatedValue>,
}

/// A holding's complete evidence packet; the model stages read only this plus
/// the engine's computed numbers.
#[derive(Debug, Clone)]
pub struct HoldingDossier {
    pub position: Position,
    /// Transient SEC recovery identity. Never serialized or rendered in prompts.
    pub earnings_issuer: Option<crate::sec::earnings::Issuer>,
    /// The issuer name off the one-per-stock FMP `/profile` lookup, kept because
    /// Schwab's `description` is often blank. `None` for a fund (its one profile
    /// read is the fund-structure leg, never an identity lookup) or an
    /// unresolved / unverified lookup.
    pub company_name: Option<String>,
    /// How this position changed since the prior run
    /// (`docs/portfolio-analysis.md` §Holdings change tracking).
    pub position_delta: PositionDelta,
    pub financials: CompanyFinancials,
    pub options_signal: OptionsSignal,
    pub profile: InvestorProfile,
    pub house_view: HouseView,
    /// The fund half for an ETF / mutual-fund holding
    /// (`docs/portfolio-analysis.md` §Asset eligibility); `None` for a stock.
    pub fund: Option<crate::portfolio::fund::FundContext>,
    /// The prior run's verdict for this holding; `None` on a debut.
    pub prior_verdict: Option<HoldingVerdict>,
    /// The prior verdict's effective analysis vintage — its `analyzed_at`, else
    /// its run's `created_at` — the retrospective's "since" anchor and the
    /// anchor-close bridge's session key (`docs/portfolio-analysis.md` §The
    /// holding verdict). A selective carry keeps the original authoring vintage,
    /// so this date and `prior_spot` stay paired. `None` on a debut.
    pub prior_vintage: Option<String>,
    /// The prior read's authoring-time spot, on the prior read's price basis —
    /// the anchor-close bridge's authoring leg. `None` on a debut or a prior
    /// audit without a quick-check basis.
    pub prior_spot: Option<f64>,
    /// The prior read's raw fiscal-period consensus EPS rows — the
    /// narrative-vs-reality read's constant-period comparator
    /// ([`crate::portfolio::engine::narrative_vs_reality`]). Empty on a debut or
    /// a basis that carried none.
    pub prior_consensus_eps_periods: Vec<crate::portfolio::engine::ConsensusEpsPeriod>,
    /// The prior run's stored engine metrics — the metric-level input delta's
    /// prior side ([`crate::portfolio::engine::metric_delta`]). `None` on a
    /// debut or a prior run without an audit row.
    pub prior_metrics: Option<crate::portfolio::engine::ComputedMetrics>,
    /// The grade-parameter version the prior letter and sub-scores were computed
    /// under; `None` without an audit row. Read beside a priced `prior_verdict`
    /// so an engine-driven letter or sub-score move is attributed to the boundary
    /// ([`crate::portfolio::engine::grade_parameter_change`]), not to evidence.
    pub prior_grade_parameter_version: Option<String>,
    /// The scenario-target parameter version the prior targets were priced under
    /// (the prior audit's `target_meta`); `None` without an audit row or a target
    /// record (a never-priced prior). Read beside a priced `prior_verdict` so an
    /// engine-driven target move is attributed to the boundary
    /// ([`crate::portfolio::engine::target_parameter_change`]), not to evidence.
    pub prior_target_parameter_version: Option<String>,
    /// The prior audit's split-bridge anchor bar
    /// ([`crate::portfolio::HoldingAudit::authoring_close`]). `None` on a debut
    /// or a prior row from a no-price exit.
    pub prior_authoring_close: Option<crate::portfolio::engine::DatedValue>,
    /// The prior run's analysis record — the holding's research memory, rendered
    /// verbatim as PRIOR ANALYSIS whatever its age (`docs/portfolio-analysis.md`
    /// §Starting parameters). `None` on a debut or a prior row carrying none.
    pub prior_analysis: Option<crate::portfolio::AnalysisRecord>,
    /// The prior row's accuracy read-through mark — the highest episode-store
    /// check id the prior review read; the self-review lists the checks above it
    /// as new (`docs/portfolio-analysis.md` §Outcome learning). `None` on a
    /// debut and on a prior that read none.
    pub prior_accuracy_read_through: Option<i64>,
    /// The prior row's fund exposure basis — the then side of a `role_risk_only`
    /// holding's realized block (`docs/portfolio-workflow.md` §Step 6e). `None`
    /// on a debut and on a prior that carried none.
    pub prior_fund_exposure: Option<crate::portfolio::fund::FundExposureBasis>,
    /// This holding's accuracy record as the run's accuracy pass left it — set
    /// after assembly by the job, read by the self-review alone. `None` where
    /// the episode store could not be read or no pass ran.
    pub accuracy: Option<crate::portfolio::outcome::SubjectAccuracy>,
    /// The listing-resolution guard's outcome for a stock
    /// (`docs/portfolio-analysis.md` §Asset eligibility). `None` on a fund; a
    /// stock always carries `Some` — offline stubs ride the trait default's
    /// `Unverified`, which proceeds with a recorded degraded input.
    pub listing: Option<crate::portfolio::listing::ListingResolution>,
    /// The item-classified 8-K sweep's outcome (`docs/portfolio-analysis.md`
    /// §Starting parameters). `None` when the leg never ran; a live stock gather
    /// always carries `Some` — `Unknown` where the sweep couldn't run, never a
    /// fabricated clear.
    pub filing_events: Option<crate::portfolio::ForensicFilingState>,
    /// The run-level commodity prints matched to this holding's sector
    /// ([`commodity_prints_for_holding`]); empty where none match or the leg
    /// never ran.
    pub commodity_context: Vec<CommodityPrint>,
    /// This holding's FINRA short-interest row (`docs/data-sources.md §FINRA`) —
    /// positioning evidence, held out of every sub-score. `None` on a fund, a
    /// symbol absent from the file (a market fact, not a gap), or a gapped
    /// file fetch.
    pub short_interest: Option<crate::finra::ShortInterestRead>,
    /// `None` on the common no-option-legs case, funds, and skipped retrievals.
    pub option_overlay: Option<OptionOverlay>,
    /// The run-level CBOE put/call backdrop — the same value on every dossier,
    /// broad-market context and never a per-name signal; `None` when the leg
    /// failed or never ran.
    pub put_call_backdrop: Option<crate::cboe::PutCallBackdrop>,
    /// Present on a stock whose sector resolved to a SPDR benchmark and whose
    /// run fetched it; the technology-event pre-flag's read-against leg.
    pub sector_benchmark: Option<BenchmarkSeries>,
    /// Symbol-scoped `news/stock` headlines — the research loop's leads, never
    /// evidence (`docs/web-research.md` §The research loop and context
    /// management). Empty on a fund, a failed or skipped fetch, and every
    /// offline stub.
    pub news_leads: Vec<crate::portfolio::research::NewsLead>,
    /// The run's session date (`YYYY-MM-DD`) — the shared holding header's
    /// "Date:" line, so every packet anchors its period labels to the analysis
    /// date rather than the model's training horizon.
    pub analysis_date: String,
    /// The data sources that contributed, for the run's audit record.
    pub sources: Vec<String>,
    /// The profile lookup's exchange, sector and industry. `None` on a fund, a
    /// skipped retrieval and an unresolved or unverified lookup.
    pub issuer: Option<IssuerProfile>,
    /// The per-holding evidence surface FETCHED VALUES renders and the engine
    /// never reads. `None` on a fund, a skipped retrieval and every offline stub
    /// whose source is not wired.
    pub evidence: Option<crate::portfolio::evidence::CompanyEvidence>,
    /// The lookback's 8-K and 8-K/A rows off the same submissions read as
    /// `filing_events`, newest first (`docs/portfolio-workflow.md` §Step 6c).
    /// Empty on a fund, a skipped retrieval, an `Unknown` sweep and every
    /// offline stub.
    pub filings_8k: Vec<crate::sec::RecentFiling>,
    /// The holding's matches against the run-level M&A feed
    /// ([`ma_matches_for_holding`]). Empty where the feed named it on neither
    /// side or never loaded.
    pub ma_matches: Vec<crate::portfolio::evidence::MaMatch>,
}

/// The prior run's carry-over for one holding, read off the latest persisted run.
#[derive(Debug, Clone)]
pub struct PriorHolding {
    pub verdict: HoldingVerdict,
    /// `None` when the run carries no audit row for the symbol.
    pub grade_parameter_version: Option<String>,
    /// `None` when the run carries no audit row for the symbol or the audit
    /// carries no target record.
    pub target_parameter_version: Option<String>,
    /// The verdict's effective analysis vintage (`analyzed_at`, else the prior
    /// run's `created_at`). A selective carry keeps the original vintage, so
    /// this date stays paired with the carried audit's authoring spot.
    pub vintage: String,
    /// The prior run's authoring-time spot (its audit's quick-check basis
    /// print). `None` where the prior audit carried no basis.
    pub spot: Option<f64>,
    /// The prior run's raw fiscal-period consensus EPS rows off the same stored
    /// basis.
    pub consensus_eps_periods: Vec<crate::portfolio::engine::ConsensusEpsPeriod>,
    /// `None` without an audit row.
    pub metrics: Option<crate::portfolio::engine::ComputedMetrics>,
    /// The prior audit's split-bridge anchor bar
    /// ([`crate::portfolio::HoldingAudit::authoring_close`]). `None` on a
    /// no-price exit's row.
    pub authoring_close: Option<crate::portfolio::engine::DatedValue>,
    /// `None` without an audit row or on a row carrying none.
    pub analysis: Option<crate::portfolio::AnalysisRecord>,
    /// `None` without an audit row or on a row that read none.
    pub accuracy_read_through: Option<i64>,
    /// `None` without an audit row or on a stock's row.
    pub fund_exposure: Option<crate::portfolio::fund::FundExposureBasis>,
}


/// Adopt the TTM statement basis where the quarterly income prints support it
/// (`docs/portfolio-analysis.md` §Starting parameters): the four newest quarters
/// sum to TTM revenue / net income / gross profit, quarters five through eight to
/// the prior-TTM revenue. One statement basis per holding: a ratio's numerator
/// and denominator must share a period basis, so the margin / growth / multiple
/// family adopts TTM wholesale or stays on the SEC annual fill, never a mix.
/// Adoption requires revenue and net income on all four newest quarters; gross
/// profit sums where every quarter carries or derives it, else stays a gap even
/// when SEC has an annual print. Returns whether the basis was adopted, so the
/// SEC merge confines itself to fallback.
pub fn apply_ttm_statement_basis(fin: &mut CompanyFinancials) -> bool {
    let adopted = adopt_ttm_statement_basis(fin);
    // Stamp which basis the values stand on at this shared choke point, so no
    // producer can set the levels without recording their basis. No quarterly
    // rows means FMP alone supports no basis (`None`), distinct from a resolved
    // `Annual` fallback. A SEC merge that follows ([`merge_financials`]) refines
    // the stamp, since only the merge knows what finally supplied the levels; a
    // caller with no merge (the quick check's `statements_refresh`) is correctly
    // described here.
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
    // Canonicalize in place first (newest-first, the latest filing winning a
    // duplicated period) so the TTM basis — and with it the letter — never
    // depends on response ordering; every statement-consuming path passes here
    // before any engine read and inherits the same order.
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
    // The four newest rows are a TTM only when consecutive — a feed gap would
    // silently sum a >12-month span into every level-vs-market-cap multiple.
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
    // Each quarter contributes its reported gross line or derives its own from
    // revenue − cost of revenue (mixing is fine, same economic line); a quarter
    // with neither gaps the whole sum.
    let ttm_gross_profit = sum4(rows, |r| {
        r.gross_profit
            .or_else(|| Some(r.revenue? - r.cost_of_revenue?))
    });
    // The prior-year window needs all eight rows contiguous — a gap anywhere
    // shifts the YoY comparison by a quarter — so growth gaps rather than
    // reading a misaligned window; the TTM basis itself stands.
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

/// Merge the keyless SEC EDGAR facts into the FMP financials and derive the
/// valuation multiples from market cap plus statement lines. On the annual
/// fallback basis SEC fills the flow lines FMP left empty; when the TTM basis
/// was adopted ([`apply_ttm_statement_basis`]) those fills are skipped so two
/// period bases never mix inside one ratio. Equity fills either way: a
/// balance-sheet instant, not a flow line.
pub fn merge_financials(
    mut fmp: CompanyFinancials,
    sec: &CompanyFacts,
    ttm_statement_basis: bool,
) -> CompanyFinancials {
    // Each fill reports whether it wrote. The basis stamp below reads only the
    // flow fills; the equity fill is a balance-sheet instant outside the
    // flow-basis rule and stamps nothing.
    let fill = |dst: &mut Option<f64>, src: Option<i64>| -> bool {
        if dst.is_none() {
            if let Some(v) = src {
                *dst = Some(v as f64);
                return true;
            }
        }
        false
    };
    let mut sec_filled_a_flow = false;
    if !ttm_statement_basis {
        sec_filled_a_flow |= fill(&mut fmp.revenue, sec.revenue);
        sec_filled_a_flow |= fill(&mut fmp.revenue_prior, sec.revenue_prior);
        sec_filled_a_flow |= fill(&mut fmp.gross_profit, sec.gross_profit);
        sec_filled_a_flow |= fill(&mut fmp.net_income, sec.net_income);
    }
    // The equity fill stamps its own source (`CompanyFinancials::equity_source`):
    // FMP's quarterly balance sheet where the FMP leg supplied equity, SEC's
    // annual print where it fell back, `None` where neither did. This is the one
    // seam that knows what finally filled it.
    let fmp_supplied_equity = fmp.total_equity.is_some();
    let sec_filled_equity = fill(&mut fmp.total_equity, sec.stockholders_equity);
    fmp.equity_source = if fmp_supplied_equity {
        Some(crate::portfolio::EquitySource::FmpQuarterly)
    } else if sec_filled_equity {
        Some(crate::portfolio::EquitySource::SecAnnual)
    } else {
        None
    };

    // Refine the basis stamp now that the fills have run (see
    // [`apply_ttm_statement_basis`]): an adopted TTM window stands; otherwise
    // `Annual` exactly when SEC filled a flow line (the basis-continuity gate
    // must cover those flows) and `None` when no flow came from SEC — a
    // balance-sheet instant standing alone must not read as an SEC annual flow
    // basis. Equity-source continuity rides `equity_source` above, not this
    // stamp: an FMP balance-sheet gap flips the source under an unchanged basis.
    if !ttm_statement_basis {
        fmp.statement_basis = sec_filled_a_flow.then_some(crate::portfolio::StatementBasis::Annual);
    }

    let derive = |num: Option<f64>, den: Option<f64>| match (num, den) {
        (Some(n), Some(d)) if d > 0.0 => Some(n / d),
        _ => None,
    };
    // The P/E derive is SIGNED (denominator ≠ 0, not > 0): a loss-maker must
    // produce a negative P/E so the engine's "a loss-maker is never cheap"
    // valuation guard is reachable; `None` would silently excuse every
    // loss-maker. P/S and P/B keep the positive-denominator derive: negative
    // revenue does not occur, and a negative-equity P/B has no engine read.
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

/// One retrieval leg's outcome as the job hands it to assembly, so the audit's
/// source list can tell a consulted leg from one the job never ran
/// (`docs/storage.md` §Local Analysis Suite Storage).
#[derive(Debug)]
pub enum LegOutcome<'a, T> {
    /// The job never ran this leg (a fund, a guard-terminal stock, a ticker with
    /// no CIK mapping). No source label.
    NotRun,
    /// The leg ran and returned nothing, or failed (the failure's gap lands in
    /// the manifest separately). Labeled as consulted, with the empty note.
    Empty,
    Got(&'a T),
}

// Manual, bound-free `Copy` / `Clone`: the enum only ever holds a shared
// reference, so it copies whether or not `T` does (the derive would demand
// `T: Copy`).
impl<T> Clone for LegOutcome<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for LegOutcome<'_, T> {}

impl<'a, T> LegOutcome<'a, T> {
    pub fn value(self) -> Option<&'a T> {
        match self {
            LegOutcome::Got(v) => Some(v),
            LegOutcome::NotRun | LegOutcome::Empty => None,
        }
    }
}

/// The profile lookup's identity fields beyond the name
/// (`docs/portfolio-workflow.md` §Step 6c).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IssuerProfile {
    pub exchange: Option<String>,
    pub sector: Option<String>,
    pub industry: Option<String>,
}

/// The stock-only evidence legs the gather hands [`assemble`]. Every field is
/// its default on a fund, a guard-terminal stock and an offline stub.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StockEvidenceLegs {
    pub issuer: Option<IssuerProfile>,
    pub evidence: Option<crate::portfolio::evidence::CompanyEvidence>,
    pub filings_8k: Vec<crate::sec::RecentFiling>,
    pub ma_matches: Vec<crate::portfolio::evidence::MaMatch>,
}

/// Every feed deal naming the symbol as acquirer or target (case-insensitive),
/// in feed order (newest first). A local lookup, never a request.
pub fn ma_matches_for_holding(
    symbol: &str,
    deals: &[crate::portfolio::evidence::MaDeal],
) -> Vec<crate::portfolio::evidence::MaMatch> {
    use crate::portfolio::evidence::{MaMatch, MaRole};
    let wanted = symbol.trim().to_ascii_uppercase();
    let is_symbol = |served: &Option<String>| {
        served
            .as_deref()
            .is_some_and(|s| s.trim().eq_ignore_ascii_case(&wanted))
    };
    let named = |name: &Option<String>, symbol: &Option<String>| {
        name.clone()
            .or_else(|| symbol.clone())
            .unwrap_or_else(|| "(counterparty unnamed)".to_string())
    };
    deals
        .iter()
        .filter_map(|deal| {
            let (role, counterparty) = if is_symbol(&deal.acquirer_symbol) {
                (
                    MaRole::Acquirer,
                    named(&deal.target_name, &deal.target_symbol),
                )
            } else if is_symbol(&deal.target_symbol) {
                (
                    MaRole::Target,
                    named(&deal.acquirer_name, &deal.acquirer_symbol),
                )
            } else {
                return None;
            };
            Some(MaMatch {
                role,
                counterparty,
                date: deal.transaction_date.clone(),
                link: deal.link.clone(),
            })
        })
        .collect()
}

/// Assemble the dossier from already-fetched pieces. Pure: the network fetches
/// happen in the job, so assembly stays deterministic and testable. A `Got` SEC
/// leg whose facts are all-`None` labels as empty too.
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    position: Position,
    position_delta: PositionDelta,
    fmp_financials: CompanyFinancials,
    sec_facts: LegOutcome<'_, CompanyFacts>,
    chain: LegOutcome<'_, OptionChain>,
    profile: InvestorProfile,
    house_view: HouseView,
    fund: Option<crate::portfolio::fund::FundContext>,
    prior: Option<PriorHolding>,
    listing: Option<crate::portfolio::listing::ListingResolution>,
    company_name: Option<String>,
    filing_events: Option<crate::portfolio::ForensicFilingState>,
    short_interest: Option<crate::finra::ShortInterestRead>,
    option_overlay: Option<OptionOverlay>,
    put_call_backdrop: Option<crate::cboe::PutCallBackdrop>,
    commodity_context: Vec<CommodityPrint>,
    sector_benchmark: Option<BenchmarkSeries>,
    news_leads: Vec<crate::portfolio::research::NewsLead>,
    analysis_date: String,
    legs: StockEvidenceLegs,
) -> HoldingDossier {
    let (
        prior_verdict,
        prior_grade_parameter_version,
        prior_target_parameter_version,
        prior_vintage,
        prior_spot,
        prior_consensus_eps_periods,
        prior_metrics,
        prior_authoring_close,
        prior_analysis,
        prior_accuracy_read_through,
        prior_fund_exposure,
    ) = match prior {
        Some(p) => (
            Some(p.verdict),
            p.grade_parameter_version,
            p.target_parameter_version,
            Some(p.vintage),
            p.spot,
            p.consensus_eps_periods,
            p.metrics,
            p.authoring_close,
            p.analysis,
            p.accuracy_read_through,
            p.fund_exposure,
        ),
        None => (
            None,
            None,
            None,
            None,
            None,
            Vec::new(),
            None,
            None,
            None,
            None,
            None,
        ),
    };
    let mut fmp_financials = fmp_financials;
    let ttm_basis = apply_ttm_statement_basis(&mut fmp_financials);
    let no_sec_facts = CompanyFacts::default();
    let financials = merge_financials(
        fmp_financials,
        sec_facts.value().unwrap_or(&no_sec_facts),
        ttm_basis,
    );
    let options_signal = chain
        .value()
        .map(crate::portfolio::engine::options_signal)
        .unwrap_or(OptionsSignal {
            put_call_volume: None,
            put_call_open_interest: None,
            implied_volatility: None,
            iv_skew: None,
        });

    // A holding whose retrieval the loop skipped never consulted the statement
    // surface, so its audit must not claim it (`docs/portfolio-analysis.md`
    // §Asset eligibility); the audit names which reason skipped it.
    let guard_terminal = matches!(
        &listing,
        Some(
            crate::portfolio::listing::ListingResolution::Unresolved
                | crate::portfolio::listing::ListingResolution::NonUs { .. }
                | crate::portfolio::listing::ListingResolution::Conflict { .. }
                | crate::portfolio::listing::ListingResolution::UnsupportedUnits { .. }
        )
    );
    // Every holding's position values come from the Schwab holdings snapshot, so
    // every branch names it. A fund's FMP pull (`FmpDataSource::fetch_fund_financials`)
    // never touches the statement endpoints, so its label says so.
    let is_fund = matches!(
        position.asset_class,
        crate::portfolio::AssetClass::Etf | crate::portfolio::AssetClass::MutualFund
    );
    let mut sources = if guard_terminal {
        vec![
            "Schwab position (holdings snapshot)".to_string(),
            "FMP company profile (listing-resolution guard)".to_string(),
        ]
    } else if !position.asset_class.is_gradeable() {
        vec![format!(
            "Schwab position ({} — not graded by the equity pipeline)",
            position.asset_class.label()
        )]
    } else if is_fund {
        vec![
            "Schwab position (holdings snapshot)".to_string(),
            "FMP fund financials (quote, EOD history, dividends)".to_string(),
        ]
    } else {
        vec![
            "Schwab position (holdings snapshot)".to_string(),
            "FMP company financials".to_string(),
        ]
    };
    // The profile lookup runs whenever a listing resolution is present, so it is
    // a consulted source on the resolved / unverified route too.
    if listing.is_some() && !guard_terminal {
        sources.push("FMP company profile (listing identity, issuer name, sector)".to_string());
    }
    // Read off the MERGED financials, since the SEC merge is what settles an
    // annual fallback; `None` records nothing (`docs/portfolio-analysis.md`
    // §Starting parameters).
    match financials.statement_basis {
        Some(crate::portfolio::StatementBasis::Ttm) => {
            sources.push("FMP TTM statement basis (four-quarter sums)".to_string())
        }
        Some(crate::portfolio::StatementBasis::Annual) => {
            sources.push("SEC annual statement basis (latest full-year lines)".to_string())
        }
        None => {}
    }
    // SEC is labeled whenever the leg was consulted, so a consulted-but-empty
    // fetch stays distinct from a leg the job never ran.
    match sec_facts {
        LegOutcome::Got(facts) if !facts.is_empty() => {
            sources.push("SEC EDGAR company facts".to_string());
        }
        LegOutcome::Got(_) | LegOutcome::Empty => {
            sources.push("SEC EDGAR company facts (empty)".to_string());
        }
        LegOutcome::NotRun => {}
    }
    // The chain likewise: a request that came back with no chain is still a
    // consulted adapter.
    match chain {
        LegOutcome::Got(_) => sources.push("Schwab option chain".to_string()),
        LegOutcome::Empty => sources.push("Schwab option chain (none returned)".to_string()),
        LegOutcome::NotRun => {}
    }
    // The filings sweep labels only where its endpoint was actually queried; an
    // unqueried `Unknown` (no CIK mapping) leaves no label.
    match &filing_events {
        Some(
            crate::portfolio::ForensicFilingState::Events { .. }
            | crate::portfolio::ForensicFilingState::Clear,
        ) => sources.push("SEC EDGAR filings (item-classified 8-K sweep)".to_string()),
        Some(crate::portfolio::ForensicFilingState::Unknown { queried: true, .. }) => {
            sources.push("SEC EDGAR filings (unavailable)".to_string())
        }
        Some(crate::portfolio::ForensicFilingState::Unknown { queried: false, .. }) | None => {}
    }
    // The targeted delta fetch labels only where it actually served or failed
    // (the stub default never makes a request).
    if let Some(o) = &option_overlay {
        sources.push("Schwab option positions (same-underlying overlay)".to_string());
        if o.delta_source_consulted {
            sources.push("Schwab option chain (overlay-delta strikes)".to_string());
        }
    }
    // The run-level commodity context deliberately does NOT label here: like
    // the house view (see below), the interpretation paths add it in `pipeline`.
    if fund.is_some() {
        sources.push(
            "FMP fund metadata (etf/info + profile structure read + weightings + sector P/E)"
                .to_string(),
        );
    }
    // The evidence surface labels wherever its pulls were issued, whatever each
    // returned. The M&A match is a local lookup, labeled by the interpretation
    // paths where it renders, like the FINRA read.
    if legs.evidence.is_some() {
        sources.push(
            "FMP company evidence (ratios, owner earnings, enterprise value, street targets \
             and grades, ratings snapshot, insider and congressional trades, float, \
             segments, splits, earnings surprises)"
                .to_string(),
        );
    }
    // The house view is deliberately NOT listed here, even though it rides every
    // dossier: whether a verdict actually consulted it is unknowable at assembly
    // (many routes through `pipeline::analyze_holding` return before any prompt),
    // and enumerating those exits here would have to be re-derived whenever a
    // new one lands. The interpretation paths, the only readers, append
    // [`HOUSE_VIEW_SOURCE`] themselves in `pipeline`.

    HoldingDossier {
        position,
        earnings_issuer: None,
        company_name,
        position_delta,
        financials,
        options_signal,
        profile,
        house_view,
        fund,
        prior_verdict,
        prior_vintage,
        prior_spot,
        prior_consensus_eps_periods,
        prior_metrics,
        prior_grade_parameter_version,
        prior_target_parameter_version,
        prior_authoring_close,
        prior_analysis,
        prior_accuracy_read_through,
        prior_fund_exposure,
        accuracy: None,
        listing,
        filing_events,
        short_interest,
        option_overlay,
        put_call_backdrop,
        commodity_context,
        sector_benchmark,
        news_leads,
        analysis_date,
        sources,
        issuer: legs.issuer,
        evidence: legs.evidence,
        filings_8k: legs.filings_8k,
        ma_matches: legs.ma_matches,
    }
}

/// The house-view freshness window, in days (`docs/portfolio-workflow.md`
/// §Step 5): a latest report older than this is omitted whole and recorded as a
/// gap rather than fed as current.
pub const HOUSE_VIEW_MAX_AGE_DAYS: i64 = 7;

/// Fail-soft: an unreadable DB or missing Markdown degrades to a thinner house
/// view, never an error. A latest report older than [`HOUSE_VIEW_MAX_AGE_DAYS`]
/// drops the whole view, summaries included; the second return reports the
/// omission for the run's data health.
pub fn load_house_view(
    conn: &Connection,
    reports_dir: &Path,
    today: chrono::NaiveDate,
) -> (HouseView, bool) {
    let with_paths = storage::list_recent_reports_with_paths(conn, HOUSE_VIEW_RECENT_REPORTS)
        .unwrap_or_default();

    // Both legs of the freshness gate are ET session dates: `created_at` is a
    // UTC instant, so reading it as a UTC prefix beside an ET `today` would
    // miscount the gap by a day across the ~8 PM ET boundary, and converting
    // only `today` would invert the error for reports written after it. An
    // unparseable stamp (never app-produced) fails soft to feeding the view.
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

/// A relative stored path joins under `reports_dir`; an absolute one is used as-is.
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

/// Pull the [`HOUSE_VIEW_SECTION_TITLES`] sections out of the report Markdown by
/// header. A section runs from its header to the next header of the
/// same-or-higher level; bounded by [`HOUSE_VIEW_CHAR_CAP`].
pub fn extract_house_view_sections(markdown: &str) -> String {
    let mut out = String::new();
    // The kept section's header level; `None` = not capturing. A deeper header
    // belongs to the section; only a same-or-higher header re-decides —
    // including a level-1 `#`, which can never start a kept section but must
    // end one.
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
    // Whole-line pushes overshoot the cap; the final cut must respect char
    // boundaries because `String::truncate` panics mid-character and report
    // prose is full of multi-byte punctuation.
    if out.len() > HOUSE_VIEW_CHAR_CAP {
        let mut cut = HOUSE_VIEW_CHAR_CAP;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out.trim().to_string()
}

/// Reads the job's already-loaded prior run (loaded once per run via
/// `store::latest_run`) rather than re-reading the store once per holding.
/// `None` on a first run or a newly-added holding.
pub fn prior_verdict_for(
    prior_run: Option<&crate::portfolio::PortfolioRun>,
    symbol: &str,
) -> Option<PriorHolding> {
    let run = prior_run?;
    let verdict = run
        .verdicts
        .iter()
        .find(|v| v.symbol.eq_ignore_ascii_case(symbol))?
        .clone();
    // The verdict's effective vintage, not the container run's `created_at`: a
    // selective carry re-persists an old verdict into a newer run, so dating off
    // the container would pair run A's spot with run B's date.
    let vintage = crate::portfolio::effective_vintage(&verdict, &run.created_at).to_string();
    let audit_row = run
        .audit
        .iter()
        .find(|a| a.symbol.eq_ignore_ascii_case(symbol));
    let (
        grade_parameter_version,
        target_parameter_version,
        spot,
        consensus_eps_periods,
        metrics,
        authoring_close,
        analysis,
        accuracy_read_through,
        fund_exposure,
    ) = match audit_row {
        Some(a) => {
            let spot = a.quick_basis.as_ref().map(|b| b.spot);
            let periods = a
                .quick_basis
                .as_ref()
                .map(|b| b.consensus_eps_periods.clone())
                .unwrap_or_default();
            (
                Some(a.grade_parameter_version.clone()),
                a.target_meta.as_ref().map(|t| t.parameter_version.clone()),
                spot,
                periods,
                Some(a.metrics.clone()),
                a.authoring_close.clone(),
                a.analysis.clone(),
                a.accuracy_read_through,
                a.fund_exposure.clone(),
            )
        }
        None => (None, None, None, Vec::new(), None, None, None, None, None),
    };
    Some(PriorHolding {
        verdict,
        grade_parameter_version,
        target_parameter_version,
        vintage,
        spot,
        consensus_eps_periods,
        metrics,
        authoring_close,
        analysis,
        accuracy_read_through,
        fund_exposure,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::engine::{QuarterlyCashFlowRow, QuarterlyIncomeRow};
    use crate::portfolio::{AssetClass, PositionChange, VerdictDisposition};
    use crate::schwab::{Holdings, Position};

    #[test]
    fn the_ma_match_names_the_holding_on_either_side_and_ignores_the_rest() {
        use crate::portfolio::evidence::{MaDeal, MaRole};
        let deal = |acq: &str, tgt: &str, date: &str| MaDeal {
            acquirer_symbol: Some(acq.into()),
            acquirer_name: Some(format!("{acq} Inc.")),
            target_symbol: Some(tgt.into()),
            target_name: Some(format!("{tgt} Corp.")),
            transaction_date: date.into(),
            link: Some("https://sec.gov/x".into()),
        };
        let deals = vec![
            deal("AAPL", "MDV", "2026-06-01"),
            deal("gnl-pe", "aapl", "2026-05-01"),
            deal("X", "Y", "2026-04-01"),
            MaDeal {
                acquirer_symbol: Some("AAPL".into()),
                target_symbol: Some("Z".into()),
                transaction_date: "2026-03-01".into(),
                ..Default::default()
            },
        ];
        let m = ma_matches_for_holding("aapl", &deals);
        assert_eq!(m.len(), 3, "{m:?}");
        assert_eq!(m[0].role, MaRole::Acquirer);
        assert_eq!(m[0].counterparty, "MDV Corp.");
        assert_eq!(m[0].link.as_deref(), Some("https://sec.gov/x"));
        assert_eq!(m[1].role, MaRole::Target);
        assert_eq!(m[1].counterparty, "gnl-pe Inc.");
        assert_eq!(
            m[2].counterparty, "Z",
            "the symbol stands in for an unnamed counterparty"
        );
        assert!(ma_matches_for_holding("Q", &deals).is_empty());
        assert!(ma_matches_for_holding("AAPL", &[]).is_empty());
    }

    #[test]
    fn assembly_labels_the_evidence_surface_where_its_pulls_issued() {
        let sources_with = |legs: StockEvidenceLegs| {
            assemble(
                Position {
                    symbol: "AAPL".into(),
                    description: "Apple Inc".into(),
                    asset_class: AssetClass::Stock,
                    quantity: 10.0,
                    cost_basis: 1_500.0,
                    market_value: 1_950.0,
                    current_price: Some(195.0),
                },
                PositionDelta::new_position(),
                fmp_only(),
                LegOutcome::<CompanyFacts>::NotRun,
                LegOutcome::<OptionChain>::NotRun,
                InvestorProfile::default_fixture(),
                HouseView::default(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Vec::new(),
                None,
                Vec::new(),
                "2026-07-28".to_string(),
                legs,
            )
            .sources
        };
        assert!(
            !sources_with(StockEvidenceLegs::default())
                .iter()
                .any(|s| s.contains("company evidence")),
            "no record, no label"
        );
        let legs = StockEvidenceLegs {
            evidence: Some(crate::portfolio::evidence::CompanyEvidence::empty("AAPL")),
            ..Default::default()
        };
        assert!(
            sources_with(legs)
                .iter()
                .any(|s| s.starts_with("FMP company evidence")),
            "an issued surface labels, whatever it returned"
        );
    }

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
    fn commodity_prints_select_by_the_profile_sector_identity() {
        let print = |label: &str, group| CommodityPrint {
            label: label.into(),
            unit: "USD".into(),
            group,
            latest: crate::portfolio::engine::DatedValue {
                date: "2026-08-18".into(),
                value: 78.4,
            },
            trailing: None,
        };
        let ctx = CommodityContext {
            prints: vec![
                print("WTI Crude Oil", CommodityGroup::Energy),
                print("Copper (IMF, monthly)", CommodityGroup::Metals),
                print("Uranium (IMF, monthly)", CommodityGroup::Uranium),
                print("Gold", CommodityGroup::Gold),
            ],
            gaps: vec![],
        };
        let labels = |sector: Option<&str>, industry: Option<&str>| -> Vec<String> {
            commodity_prints_for_holding(&ctx, sector, industry)
                .into_iter()
                .map(|p| p.label)
                .collect()
        };
        assert_eq!(labels(Some("Energy"), None), vec!["WTI Crude Oil".to_string()]);
        // Coal gets no fabricated proxy because the catalog carries no coal print.
        assert_eq!(
            labels(Some("Energy"), Some("Uranium")),
            vec!["Uranium (IMF, monthly)".to_string()]
        );
        assert_eq!(
            labels(Some("Basic Materials"), Some("Uranium Mining")),
            vec!["Uranium (IMF, monthly)".to_string()]
        );
        assert!(labels(Some("Energy"), Some("Thermal Coal")).is_empty());
        assert!(labels(Some("Energy"), Some("Coking Coal")).is_empty());
        // Gold only on a gold-linked industry: a steel holding carries no gold evidence.
        assert_eq!(
            labels(Some("Basic Materials"), Some("Steel")),
            vec!["Copper (IMF, monthly)".to_string()]
        );
        assert_eq!(
            labels(Some("Basic Materials"), Some("Gold")),
            vec!["Copper (IMF, monthly)".to_string(), "Gold".to_string()]
        );
        assert_eq!(
            labels(Some("Basic Materials"), Some("Other Precious Metals & Mining")),
            vec!["Copper (IMF, monthly)".to_string(), "Gold".to_string()]
        );
        assert!(labels(Some("Technology"), None).is_empty());
        assert!(labels(None, None).is_empty());
    }

    #[test]
    fn house_view_older_than_a_week_is_omitted_whole_and_flagged() {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        insert_house_view_report(&conn, "r-old", "2026-07-20T14:00:00Z");
        // 8 ET days later. The stamp is mid-session (10 AM ET) so the report's ET
        // date is unambiguously its own UTC date — a midnight-UTC stamp would
        // belong to the PRIOR ET session.
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
        // The run is 9 PM ET on 2026-07-28: seven ET days later, but its UTC
        // instant has already rolled to the 29th.
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
        // 9 PM ET on 2026-07-21: the UTC stamp says the 22nd, the ET session the
        // 21st. Converting only the run's `today` would read this report a day
        // younger than it is; it ages from the 21st and is exactly 8 ET days stale.
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
    fn merge_stamps_which_balance_sheet_supplied_the_equity() {
        use crate::portfolio::EquitySource;
        let sec = CompanyFacts {
            stockholders_equity: Some(60_000_000_000),
            ..Default::default()
        };
        let mut fmp = fmp_only();
        fmp.total_equity = Some(50_000_000_000.0);
        let merged = merge_financials(fmp, &sec, true);
        assert_eq!(merged.total_equity, Some(50_000_000_000.0));
        assert_eq!(merged.equity_source, Some(EquitySource::FmpQuarterly));
        // SEC fills on either flow basis: the instant sits outside the flow-basis rule.
        for ttm in [true, false] {
            let merged = merge_financials(fmp_only(), &sec, ttm);
            assert_eq!(merged.total_equity, Some(60_000_000_000.0));
            assert_eq!(
                merged.equity_source,
                Some(EquitySource::SecAnnual),
                "ttm={ttm}"
            );
        }
        let merged = merge_financials(fmp_only(), &CompanyFacts::default(), false);
        assert_eq!(merged.total_equity, None);
        assert_eq!(merged.equity_source, None);
    }

    #[test]
    fn merge_keeps_fmp_supplied_multiples_and_leaves_missing_inputs_as_gaps() {
        let mut fmp = fmp_only();
        fmp.pe_ratio = Some(28.0);
        let sec = CompanyFacts::default();
        let merged = merge_financials(fmp, &sec, false);
        assert_eq!(merged.pe_ratio, Some(28.0), "FMP value not overwritten");
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
                    research_and_development: None,
                    income_before_tax: None,
                    income_tax_expense: None,
                }
            })
            .collect()
    }

    #[test]
    fn a_zero_row_quarterly_response_with_a_sec_fallback_stamps_the_annual_basis() {
        use crate::portfolio::StatementBasis;
        // An EMPTY quarterly set adopts no TTM window, so SEC's annual facts fill
        // the levels and those levels must stamp `Annual`, not `None`. The
        // multiples key their observation on the marks' trading day, not a
        // statement print, so they resolve even with zero quarterly rows.
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
        // Nothing supplied a statement level, so there is no basis. A fund reaches
        // the engine this way, and `None` must keep meaning this rather than
        // doubling as annual.
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
    fn an_equity_instant_alone_carries_no_flow_basis_and_a_sec_flow_fill_is_annual() {
        use crate::portfolio::StatementBasis;
        // Thin quarters beside FMP's own balance sheet, SEC never run: an instant
        // on no flow basis is `None`, never SEC provenance for a level SEC never
        // supplied.
        let mut fin = fmp_only();
        fin.quarterly_income = quarters(2, true, false);
        fin.total_equity = Some(60_000_000_000.0);
        assert!(!apply_ttm_statement_basis(&mut fin));
        let merged = merge_financials(fin, &CompanyFacts::default(), false);
        assert_eq!(merged.statement_basis, None);
        assert_eq!(
            merged.total_equity,
            Some(60_000_000_000.0),
            "the instant stands"
        );
        // SEC supplying only equity fills an instant, not a flow, so the annual
        // flow basis is not stamped.
        let mut fin = fmp_only();
        fin.quarterly_income = quarters(2, true, false);
        assert!(!apply_ttm_statement_basis(&mut fin));
        let sec = CompanyFacts {
            stockholders_equity: Some(60_000_000_000),
            ..CompanyFacts::default()
        };
        let merged = merge_financials(fin, &sec, false);
        assert_eq!(merged.statement_basis, None);
        assert_eq!(
            merged.total_equity,
            Some(60_000_000_000.0),
            "the SEC instant still fills"
        );
        // A single SEC flow line is the annual basis — that flow is what the gate
        // must cover.
        let mut fin = fmp_only();
        fin.quarterly_income = quarters(2, true, false);
        assert!(!apply_ttm_statement_basis(&mut fin));
        let sec = CompanyFacts {
            net_income: Some(100_000_000_000),
            ..CompanyFacts::default()
        };
        let merged = merge_financials(fin, &sec, false);
        assert_eq!(merged.statement_basis, Some(StatementBasis::Annual));
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
        let mut fin = fmp_only();
        let mut rows = quarters(5, true, false);
        rows.remove(1); // drop 2026-03-31: newest four now span 2025-06-30..2026-06-30
        fin.quarterly_income = rows;
        assert!(!apply_ttm_statement_basis(&mut fin), "gapped window must not adopt");
        assert!(fin.revenue.is_none(), "no partial fill on the failed adoption");
    }

    #[test]
    fn ttm_prior_window_gaps_on_a_non_contiguous_eight_quarter_run() {
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
        // Two quarters report the gross line, two derive theirs from cost of revenue.
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
        assert!(!sections.contains("Dow up"), "{sections}");
        assert!(!sections.contains("a bullet"), "{sections}");
    }

    #[test]
    fn extract_honors_the_section_level_contract() {
        // A `###` sub-heading inside a kept `##` section belongs to it; a level-1
        // `#` heading ends capture.
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
        // Header (24 bytes) + "x" puts the em-dash run at byte 25; the cap at
        // 6000 lands (6000 − 25) = 5975 bytes in, and 5975 mod 3 = 2 is mid-dash
        // by construction.
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
                    delta: None,
                },
                OptionQuote {
                    kind: OptionKind::Put,
                    strike: 195.0,
                    expiry: "2026-07-17".into(),
                    volume: 1500.0,
                    open_interest: 6000.0,
                    implied_volatility: Some(0.31),
                    delta: None,
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
            LegOutcome::Got(&sec),
            LegOutcome::Got(&chain),
            InvestorProfile::default_fixture(),
            HouseView::default(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            "2026-07-28".to_string(),
            StockEvidenceLegs::default(),
        );
        assert!(dossier.sources.iter().any(|s| s.contains("FMP")));
        assert!(dossier.sources.iter().any(|s| s.contains("SEC")));
        assert!(dossier.sources.contains(&"Schwab option chain".to_string()));
        assert!(dossier.options_signal.put_call_volume.unwrap() > 1.0);
        assert!(dossier.prior_verdict.is_none(), "new holding");
    }

    #[test]
    fn option_chain_leg_is_labeled_whenever_consulted_and_none_is_distinct_from_not_run() {
        // `docs/storage.md`: "the source labels name every adapter the holding consulted".
        let chain_labels = |sources: Vec<String>| -> Vec<String> {
            sources.into_iter().filter(|s| s.contains("option chain")).collect()
        };
        let chain = OptionChain {
            underlying: "AAPL".into(),
            underlying_price: Some(195.0),
            contracts: vec![],
        };
        assert_eq!(
            chain_labels(stock_sources_with_chain(LegOutcome::Got(&chain))),
            vec!["Schwab option chain".to_string()]
        );
        assert_eq!(
            chain_labels(stock_sources_with_chain(LegOutcome::Empty)),
            vec!["Schwab option chain (none returned)".to_string()]
        );
        assert!(chain_labels(stock_sources_with_chain(LegOutcome::NotRun)).is_empty());
    }

    #[test]
    fn a_gradeable_holding_names_the_schwab_position_snapshot() {
        let sources = stock_sources(LegOutcome::NotRun, None);
        assert_eq!(
            sources,
            vec![
                "Schwab position (holdings snapshot)".to_string(),
                "FMP company financials".to_string(),
            ],
            "{sources:?}"
        );
        // A fund's FMP pull is `fetch_fund_financials` — never the statement /
        // consensus endpoints — so its label says so.
        let fund = assemble(
            Position {
                symbol: "QQQ".into(),
                description: "Invesco QQQ".into(),
                asset_class: AssetClass::Etf,
                quantity: 10.0,
                cost_basis: 3_000.0,
                market_value: 4_000.0,
                current_price: Some(400.0),
            },
            PositionDelta::new_position(),
            fmp_only(),
            LegOutcome::NotRun,
            LegOutcome::NotRun,
            InvestorProfile::default_fixture(),
            HouseView::default(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            "2026-07-28".to_string(),
            StockEvidenceLegs::default(),
        )
        .sources;
        assert_eq!(
            fund,
            vec![
                "Schwab position (holdings snapshot)".to_string(),
                "FMP fund financials (quote, EOD history, dividends)".to_string(),
            ],
            "{fund:?}"
        );
    }

    #[test]
    fn assembly_never_claims_the_house_view_because_it_cannot_know() {
        let house_view = HouseView {
            recent_summaries: Vec::new(),
            latest_sections: Some("## Market Signal Thesis\nrisk-on.".into()),
        };
        let position = |asset_class| Position {
            symbol: "SWVXX".into(),
            description: "Schwab Value Advantage Money Fund".into(),
            asset_class,
            quantity: 5_000.0,
            cost_basis: 5_000.0,
            market_value: 5_000.0,
            current_price: Some(1.0),
        };
        let assemble_with = |asset_class| {
            assemble(
                position(asset_class),
                PositionDelta::new_position(),
                fmp_only(),
                LegOutcome::NotRun,
                LegOutcome::NotRun,
                InvestorProfile::default_fixture(),
                house_view.clone(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Vec::new(),
                None,
                Vec::new(),
                "2026-07-28".to_string(),
                StockEvidenceLegs::default(),
            )
            .sources
        };

        assert_eq!(
            assemble_with(AssetClass::Cash),
            vec!["Schwab position (cash — not graded by the equity pipeline)".to_string()],
        );

        // The interpretation paths add it themselves — pinned in `pipeline`.
        assert!(
            !assemble_with(AssetClass::Stock)
                .iter()
                .any(|src| src.contains("house view")),
            "assembly never claims the house view — it cannot know"
        );
    }

    /// Over the FMP-only financials.
    fn stock_sources_full(
        sec_facts: LegOutcome<'_, CompanyFacts>,
        chain: LegOutcome<'_, OptionChain>,
        listing: Option<crate::portfolio::listing::ListingResolution>,
    ) -> Vec<String> {
        stock_sources_assembled(fmp_only(), sec_facts, chain, listing)
    }

    fn stock_sources_assembled(
        fin: CompanyFinancials,
        sec_facts: LegOutcome<'_, CompanyFacts>,
        chain: LegOutcome<'_, OptionChain>,
        listing: Option<crate::portfolio::listing::ListingResolution>,
    ) -> Vec<String> {
        let position = Position {
            symbol: "AAPL".into(),
            description: "Apple Inc".into(),
            asset_class: AssetClass::Stock,
            quantity: 10.0,
            cost_basis: 1_000.0,
            market_value: 1_950.0,
            current_price: Some(195.0),
        };
        assemble(
            position,
            PositionDelta::new_position(),
            fin,
            sec_facts,
            chain,
            InvestorProfile::default_fixture(),
            HouseView::default(),
            None,
            None,
            listing,
            None,
            None,
            None,
            None,
            None,
            Vec::new(),
            None,
            Vec::new(),
            "2026-07-28".to_string(),
            StockEvidenceLegs::default(),
        )
        .sources
    }

    /// No chain leg run.
    fn stock_sources(
        sec_facts: LegOutcome<'_, CompanyFacts>,
        listing: Option<crate::portfolio::listing::ListingResolution>,
    ) -> Vec<String> {
        stock_sources_full(sec_facts, LegOutcome::NotRun, listing)
    }

    /// No SEC leg run, no listing.
    fn stock_sources_with_chain(chain: LegOutcome<'_, OptionChain>) -> Vec<String> {
        stock_sources_full(LegOutcome::NotRun, chain, None)
    }

    #[test]
    fn sources_line_names_the_adopted_statement_basis() {
        let basis_labels = |sources: Vec<String>| -> Vec<String> {
            sources
                .into_iter()
                .filter(|s| s.contains("statement basis"))
                .collect()
        };
        let mut ttm = fmp_only();
        ttm.quarterly_income = quarters(8, true, false);
        assert_eq!(
            basis_labels(stock_sources_assembled(
                ttm,
                LegOutcome::NotRun,
                LegOutcome::NotRun,
                None
            )),
            vec!["FMP TTM statement basis (four-quarter sums)".to_string()]
        );
        let facts = CompanyFacts {
            revenue: Some(400_000_000_000),
            revenue_prior: Some(360_000_000_000),
            net_income: Some(100_000_000_000),
            ..CompanyFacts::default()
        };
        assert_eq!(
            basis_labels(stock_sources_assembled(
                fmp_only(),
                LegOutcome::Got(&facts),
                LegOutcome::NotRun,
                None
            )),
            vec!["SEC annual statement basis (latest full-year lines)".to_string()]
        );
        // An equity instant beside thin quarters, SEC never run: no basis, so no
        // label claims SEC provenance.
        let mut instant_only = fmp_only();
        instant_only.quarterly_income = quarters(2, true, false);
        instant_only.total_equity = Some(60_000_000_000.0);
        assert!(basis_labels(stock_sources_assembled(
            instant_only,
            LegOutcome::NotRun,
            LegOutcome::NotRun,
            None
        ))
        .is_empty());
        // An equity-only SEC fill is the same instant-only shape.
        let equity_only = CompanyFacts {
            stockholders_equity: Some(60_000_000_000),
            ..CompanyFacts::default()
        };
        let mut thin = fmp_only();
        thin.quarterly_income = quarters(2, true, false);
        assert!(basis_labels(stock_sources_assembled(
            thin,
            LegOutcome::Got(&equity_only),
            LegOutcome::NotRun,
            None
        ))
        .is_empty());
        assert!(basis_labels(stock_sources_assembled(
            fmp_only(),
            LegOutcome::NotRun,
            LegOutcome::NotRun,
            None
        ))
        .is_empty());
    }

    #[test]
    fn sec_leg_is_labeled_whenever_consulted_and_empty_is_distinct_from_not_run() {
        let sec_labels = |sources: Vec<String>| -> Vec<String> {
            sources
                .into_iter()
                .filter(|s| s.contains("SEC EDGAR"))
                .collect()
        };
        let facts = CompanyFacts {
            revenue: Some(400_000_000_000),
            ..CompanyFacts::default()
        };
        assert_eq!(
            sec_labels(stock_sources(LegOutcome::Got(&facts), None)),
            vec!["SEC EDGAR company facts".to_string()]
        );
        // Empty facts from a queried endpoint and the bare `Empty` outcome label alike.
        assert_eq!(
            sec_labels(stock_sources(LegOutcome::Got(&CompanyFacts::default()), None)),
            vec!["SEC EDGAR company facts (empty)".to_string()]
        );
        assert_eq!(
            sec_labels(stock_sources(LegOutcome::Empty, None)),
            vec!["SEC EDGAR company facts (empty)".to_string()]
        );
        // Never run is also the no-CIK case: the "no CIK mapping" gap recorded by
        // the job's `sec_company_facts` is the trace it leaves.
        assert!(sec_labels(stock_sources(LegOutcome::NotRun, None)).is_empty());
    }

    #[test]
    fn profile_lookup_is_a_source_on_the_common_resolved_route_too() {
        use crate::portfolio::listing::ListingResolution;
        let identity = "FMP company profile (listing identity, issuer name, sector)".to_string();
        let guard = "FMP company profile (listing-resolution guard)".to_string();

        let resolved = stock_sources(LegOutcome::NotRun, Some(ListingResolution::SupportedUs));
        assert!(resolved.contains(&identity), "{resolved:?}");
        assert!(resolved.contains(&"FMP company financials".to_string()));
        assert!(!resolved.contains(&guard));
        // Unverified (an FMP profile gap): the lookup still ran and is named.
        let unverified = stock_sources(
            LegOutcome::NotRun,
            Some(ListingResolution::Unverified {
                detail: "FMP profile unavailable".into(),
            }),
        );
        assert!(unverified.contains(&identity), "{unverified:?}");
        // Guard-terminal: no FMP financials pull ran.
        let terminal = stock_sources(LegOutcome::NotRun, Some(ListingResolution::Unresolved));
        assert_eq!(
            terminal,
            vec!["Schwab position (holdings snapshot)".to_string(), guard]
        );
        assert!(
            !stock_sources(LegOutcome::NotRun, None).iter().any(|s| s.contains("company profile")),
            "{:?}",
            stock_sources(LegOutcome::NotRun, None)
        );
    }

    #[test]
    fn ttm_basis_resolves_conflicting_duplicate_periods_to_the_latest_filing() {
        // The newest quarter served twice (a restatement), in either arrival order.
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
        // An oldest-first feed: the basis must read the true newest four and
        // leave both statement vecs canonical for every later reader.
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
                depreciation_and_amortization: None,
            },
            QuarterlyCashFlowRow {
                period_end: "2026-06-30".into(),
                filing_date: None,
                free_cash_flow: Some(3.0),
                operating_cash_flow: None,
                capex: None,
                depreciation_and_amortization: None,
            },
        ];
        assert!(apply_ttm_statement_basis(&mut fin));
        assert_eq!(fin.revenue, Some(394.0));
        assert_eq!(fin.revenue_prior, Some(378.0));
        assert_eq!(fin.quarterly_income[0].period_end, "2026-06-30");
        assert_eq!(fin.quarterly_income[7].period_end, "2024-09-30");
        assert_eq!(fin.quarterly_cash_flow[0].period_end, "2026-06-30");
    }

    #[test]
    fn statement_rows_canonicalize_even_when_the_basis_is_not_adopted() {
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
        assert!(prior_verdict_for(None, "AAPL").is_none());

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
                basis_move: None,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                analyzed_at: None,
                action_source: Default::default(),
                side_reversed: false,
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                failed_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: Default::default(),
                overview: String::new(),
            },
            audit: vec![],
            rate_prints: Default::default(),
            failed_holdings: Vec::new(),
            accuracy: Default::default(),
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let latest = crate::portfolio::store::latest_run(&conn).unwrap();
        let prior = prior_verdict_for(latest.as_ref(), "aapl").expect("case-insensitive match");
        assert_eq!(prior.verdict.symbol, "AAPL");
        // No audit row for the symbol -> no stamp to read, on either axis.
        assert_eq!(prior.grade_parameter_version, None);
        assert_eq!(prior.target_parameter_version, None);
    }

    #[test]
    fn prior_verdict_lookup_carries_the_stamped_grade_and_target_parameter_versions() {
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
                basis_move: None,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                analyzed_at: None,
                action_source: Default::default(),
                side_reversed: false,
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                failed_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: Default::default(),
                overview: String::new(),
            },
            audit: vec![crate::portfolio::HoldingAudit {
                research: None,
                analysis: Some(crate::portfolio::AnalysisRecord {
                    text: "The analysis.".into(),
                    written: "2026-08-03".into(),
                    anchor: None,
                }),
                review: None,
                accuracy_read_through: Some(41),
                symbol: "AAPL".into(),
                metrics: Default::default(),
                sources: vec![],
                model_ids: vec![],
                prompt_version: crate::portfolio::PROMPT_VERSION.to_string(),
                evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.to_string(),
                degraded_inputs: vec![],
                action_annotations: vec![],
                target_meta: Some(crate::portfolio::engine::TargetMeta {
                    parameter_version: "targets-v4".into(),
                    ..Default::default()
                }),
                grade_parameter_version: "grade-v2".into(),
                factors: None,
                quick_basis: None,
                authoring_close: None,
                fund_exposure: Some(crate::portfolio::fund::FundExposureBasis {
                    class_label: "bond fund".into(),
                    expense_ratio: Some(0.0003),
                    us_share: None,
                    top_sector: None,
                    structural_flag: false,
                    nav: Some(73.5),
                }),
                pre_profit: None,
                hurdle: None,
                forensic: None,
                soft_forensic: None,
                tech_event_pre_flag: None,
                short_interest: None,
                implied_expectations: None,
                narrative: None,
                option_overlay: None,
            }],
            rate_prints: Default::default(),
            failed_holdings: Vec::new(),
            accuracy: Default::default(),
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let latest = crate::portfolio::store::latest_run(&conn).unwrap();
        let prior = prior_verdict_for(latest.as_ref(), "AAPL").expect("verdict present");
        assert_eq!(prior.grade_parameter_version.as_deref(), Some("grade-v2"));
        assert_eq!(prior.target_parameter_version.as_deref(), Some("targets-v4"));
        let analysis = prior.analysis.as_ref().expect("the analysis record");
        assert_eq!((analysis.text.as_str(), analysis.written.as_str()), ("The analysis.", "2026-08-03"));
        assert_eq!(prior.accuracy_read_through, Some(41));
        assert_eq!(prior.fund_exposure.as_ref().and_then(|b| b.nav), Some(73.5));
        // No `analyzed_at` on the verdict -> the vintage falls back to the
        // container run's `created_at`.
        assert_eq!(prior.vintage, "2026-08-03T00:00:00Z");
    }

    #[test]
    fn prior_verdict_lookup_dates_a_selective_carry_by_its_own_vintage() {
        // A selective run re-persists a carried verdict (its `analyzed_at` intact)
        // into a newer container run; the container's `created_at` would pair run
        // A's spot with run B's date.
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
                basis_move: None,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                analyzed_at: Some("2026-07-29T12:00:00Z".into()),
                action_source: Default::default(),
                side_reversed: false,
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                failed_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: Default::default(),
                overview: String::new(),
            },
            audit: vec![],
            rate_prints: Default::default(),
            failed_holdings: Vec::new(),
            accuracy: Default::default(),
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let latest = crate::portfolio::store::latest_run(&conn).unwrap();
        let prior = prior_verdict_for(latest.as_ref(), "AAPL").expect("verdict present");
        assert_eq!(prior.vintage, "2026-07-29T12:00:00Z");
    }

    fn opt_row(symbol: &str, quantity: f64) -> Position {
        Position {
            symbol: symbol.into(),
            description: String::new(),
            asset_class: AssetClass::OptionContract,
            quantity,
            cost_basis: 0.0,
            market_value: 0.0,
            current_price: None,
        }
    }

    const CALL_210: &str = "AAPL  270115C00210000";
    const PUT_180: &str = "AAPL  270115P00180000";

    #[test]
    fn overlay_classifies_the_single_strategy_shapes_with_coverage_and_delta() {
        let with_delta = |c: &crate::schwab::OccContract| {
            Some(match c.kind {
                crate::schwab::OptionKind::Call => 0.40,
                crate::schwab::OptionKind::Put => -0.30,
            })
        };
        // Covered call: 2 short calls fully covered by 200 shares.
        let rows = [opt_row(CALL_210, -2.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(200.0, &refs, with_delta, true).unwrap();
        assert_eq!(o.class, OverlayClass::CoveredCall);
        assert_eq!(o.coverage_ratio, Some(1.0));
        assert_eq!(o.net_delta, Some(-80.0), "{o:?}"); // −2 × 100 × 0.40
        assert!(o.gaps.is_empty(), "{o:?}");
        // Protective put on the same book.
        let rows = [opt_row(PUT_180, 2.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(200.0, &refs, with_delta, true).unwrap();
        assert_eq!(o.class, OverlayClass::ProtectivePut);
        assert_eq!(o.coverage_ratio, Some(1.0));
        // Collar: the covered fraction is the narrower side.
        let rows = [opt_row(CALL_210, -1.0), opt_row(PUT_180, 2.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(200.0, &refs, with_delta, true).unwrap();
        assert_eq!(o.class, OverlayClass::Collar);
        assert_eq!(o.coverage_ratio, Some(0.5));
        // A zero-net row (fully offset contracts) is never a leg or an overlay.
        assert!(assemble_option_overlay(200.0, &[], with_delta, true).is_none());
        let rows = [opt_row(CALL_210, 0.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        assert!(assemble_option_overlay(200.0, &refs, with_delta, true).is_none());
        // A zero row beside a real leg drops silently; the real leg classifies.
        let rows = [opt_row(CALL_210, -1.0), opt_row(PUT_180, 0.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(200.0, &refs, with_delta, true).unwrap();
        assert_eq!(o.legs.len(), 1, "{o:?}");
        assert_eq!(o.class, OverlayClass::CoveredCall);
    }

    #[test]
    fn overlay_never_reads_naked_or_unrecognized_legs_as_covered() {
        let no_delta = |_: &crate::schwab::OccContract| None;
        // 2 short calls over 100 shares: naked.
        let rows = [opt_row(CALL_210, -2.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(100.0, &refs, no_delta, false).unwrap();
        assert_eq!(o.class, OverlayClass::Other, "{o:?}");
        assert!(o.coverage_ratio.is_none());
        assert!(o.gaps.iter().any(|g| g.contains("naked")), "{o:?}");
        // An unrecognized symbol's delta is a gap, so the net delta is None.
        let rows = [opt_row("AAPL WEIRD LEG", 1.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(100.0, &refs, no_delta, false).unwrap();
        assert_eq!(o.class, OverlayClass::Other);
        assert!(o.net_delta.is_none());
        assert!(o.gaps.iter().any(|g| g.contains("unrecognized")), "{o:?}");
        // A multi-leg outside the three shapes (long call + short put) is
        // `other` with its net delta where deltas resolve.
        let with_delta = |c: &crate::schwab::OccContract| {
            Some(match c.kind {
                crate::schwab::OptionKind::Call => 0.40,
                crate::schwab::OptionKind::Put => -0.30,
            })
        };
        let rows = [opt_row(CALL_210, 1.0), opt_row(PUT_180, -1.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(100.0, &refs, with_delta, true).unwrap();
        assert_eq!(o.class, OverlayClass::Other);
        assert_eq!(o.net_delta, Some(70.0)); // +100×0.40 − 100×(−0.30)
        // A non-long underlying never classifies a strategy.
        let rows = [opt_row(CALL_210, -1.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(-100.0, &refs, with_delta, true).unwrap();
        assert_eq!(o.class, OverlayClass::Other);
        // A gapped delta on a classified shape: the class holds, the net delta
        // does not.
        let rows = [opt_row(CALL_210, -1.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(100.0, &refs, |_: &crate::schwab::OccContract| None, true)
            .unwrap();
        assert_eq!(o.class, OverlayClass::CoveredCall);
        assert!(o.net_delta.is_none());
        assert!(o.gaps.iter().any(|g| g.contains("delta unavailable")), "{o:?}");
    }
}
