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

/// The audit's source line for the Market Signal house view. Appended by the
/// interpretation paths in `pipeline`, never by dossier assembly — see the note at the
/// end of [`assemble`].
pub const HOUSE_VIEW_SOURCE: &str = "Market Signal Report (house view)";

/// The Market Signal house view loaded as a read-only shared input
/// (`docs/portfolio-analysis.md`). It enters deterministically — recent report
/// summaries plus the latest report's relevant prose sections — never via the
/// report's vector memory (which a local job cannot read anyway: different namespace
/// and embedder, see `crate::vector_memory::MemoryNamespace`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HouseView {
    pub recent_summaries: Vec<ReportSummary>,
    /// The latest report's Thesis / Investment Strategy / Forward Outlook prose,
    /// concatenated; `None` when no report exists or none could be read.
    pub latest_sections: Option<String>,
}

/// How many days each run-level commodity window covers (drafted): wide enough
/// for ~13 monthly IMF prints and a ~1-year daily trend base, one date-ranged
/// request per series.
pub const COMMODITY_WINDOW_DAYS: i64 = 400;

/// Which commodity sleeve a print belongs to — the deterministic key the
/// per-holding selection reads (`docs/data-sources.md §Portfolio Analysis —
/// endpoint surface`: energy series for energy-linked holdings, the IMF metals
/// plus gold for materials-linked ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CommodityGroup {
    Energy,
    Metals,
    Gold,
}

/// One run-level commodity price read: the latest print on the series' own
/// published level basis (never the rate normalization), plus the window's
/// earliest print as a trend base. Each carries its observation date so the
/// model reads it as-of — a monthly IMF series lags by design, and data honesty
/// shows the lag rather than presenting the print as current.
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

/// The run-level commodity context (`docs/portfolio-workflow.md` §Step 5):
/// fetched **once per run and shared across every holding** — FRED daily energy
/// plus the suite-shared monthly IMF metals, and gold via FMP `quote` `GCUSD` —
/// each series fail-soft to a typed gap. Empty prints + empty gaps = the leg
/// never ran (an offline stub).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CommodityContext {
    pub prints: Vec<CommodityPrint>,
    pub gaps: Vec<String>,
}

/// Select the prints that ride one holding's dossier, by the holding's FMP
/// profile identity: sector Energy → the energy sleeve; sector Basic
/// Materials → the metals sleeve; and **gold on the industry label alone** (a
/// gold / precious-metals industry) — never the whole Basic Materials sector,
/// so a steel or chemicals holding carries no gold evidence. Any other
/// identity — or none — carries no commodity block; the context is
/// commodity-linked evidence, not a universal macro feed.
pub fn commodity_prints_for_holding(
    ctx: &CommodityContext,
    sector: Option<&str>,
    industry: Option<&str>,
) -> Vec<CommodityPrint> {
    let mut groups: Vec<CommodityGroup> = Vec::new();
    match sector {
        Some("Energy") => groups.push(CommodityGroup::Energy),
        Some("Basic Materials") => groups.push(CommodityGroup::Metals),
        _ => {}
    }
    if industry
        .map(|i| {
            let l = i.to_ascii_lowercase();
            l.contains("gold") || l.contains("precious metals")
        })
        .unwrap_or(false)
    {
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
    /// The contract's delta off the targeted chain fetch — `None` is the typed
    /// gap (a failed fetch or the sentinel). A standalone option never becomes
    /// a leg at all (ruled 2026-08-21 — absence, not a recorded gap).
    pub delta: Option<f64>,
}

/// The typed same-underlying option overlay (`docs/portfolio-workflow.md`
/// §Step 6a): the holding's option legs off the Step-2 pull, linked by the
/// deterministic OCC symbol decode, classified, with the coverage ratio and
/// the net share-equivalent delta. Evidence for the verdict and the action
/// call — the overlay changes what the right action is — never a grade input.
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

/// Build the typed overlay from the holding's same-underlying option rows.
/// `shares` is the holding's netted quantity; `delta_for` resolves a decoded
/// contract to its chain delta (the job's targeted-fetch lookup). `None` when
/// no option rows exist — the common case, no overlay to carry.
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
        // A zero-net row is no economic exposure — no leg (defensive; the job's
        // collector already excludes them).
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

    // Classification (drafted rules): single-strategy shapes only; anything
    // else — an unrecognized leg, a non-long underlying, a naked short call, or
    // extra legs — is `other`. A short call is covered only up to the held
    // shares; any excess is naked and must never read as covered.
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

/// One sector benchmark's dated closes (FMP dated EOD — the identity table in
/// `docs/data-sources.md §Financial Modeling Prep`), fetched run-level and
/// memoized per symbol; the input delta's technology-event pre-flag reads it.
#[derive(Debug, Clone)]
pub struct BenchmarkSeries {
    pub symbol: String,
    pub closes: Vec<crate::portfolio::engine::DatedValue>,
}

/// A holding's complete evidence packet, assembled deterministically. The pipeline's
/// model stages read only this (plus the engine's computed numbers), so interpretation
/// reasons over evidence, not over a gathering transcript.
#[derive(Debug, Clone)]
pub struct HoldingDossier {
    pub position: Position,
    /// The issuer name off the same one-per-stock FMP `/profile` lookup the listing
    /// guard reads — kept because Schwab's `description` is often blank, which renders
    /// the prompt header as `HOLDING: PSX ()` and leaves the model guessing at the
    /// issuer (`docs/verification/2026-08-10-big-run-attempt-1.md` §Finding 4).
    /// `None` for a fund (its one profile read is the fund-structure leg —
    /// `isFund` + description for closed-end detection, never an identity
    /// lookup) or an unresolved / unverified lookup.
    pub company_name: Option<String>,
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
    /// The prior read's stored NTM consensus-EPS mid (the same quick-check
    /// basis) — the narrative-vs-reality read's revision comparator
    /// ([`crate::portfolio::engine::narrative_vs_reality`]). `None` on a debut
    /// or a basis that carried none.
    pub prior_consensus_eps_mid: Option<f64>,
    /// The prior run's matured outcome-window lines for this symbol (deterministic,
    /// engine-computed) — the scored ground the retrospective reads against, where
    /// any windows have matured. Empty on a debut or before any window matures.
    pub prior_matured_notes: Vec<String>,
    /// The prior run's stored engine metrics (from the audit row) — the
    /// metric-level input delta's prior side ([`crate::portfolio::engine::metric_delta`]).
    /// `None` on a debut or a prior run without an audit row.
    pub prior_metrics: Option<crate::portfolio::engine::ComputedMetrics>,
    /// The grade-parameter version the prior verdict's letter and sub-scores were
    /// computed under (from the prior run's audit row; `None` when the run carries
    /// no audit row for the symbol). Meaningful only beside a priced `prior_verdict` — the
    /// input delta and the interpretation prompt read the boundary it sits across
    /// on the holding's branch ([`crate::portfolio::engine::grade_parameter_change`])
    /// so an engine-driven letter or sub-score move is attributed to that boundary,
    /// not to evidence.
    pub prior_grade_parameter_version: Option<String>,
    /// The scenario-target parameter version the prior verdict's targets were
    /// priced under (from the prior audit row's `target_meta`; `None` when the
    /// run carries no audit row for the symbol or the audit carries no target
    /// record — a never-priced prior). Meaningful only beside a priced
    /// `prior_verdict` — the input delta and the interpretation prompt read the
    /// horizons its boundary can have moved on the holding's branch
    /// ([`crate::portfolio::engine::target_parameter_change`]) so an
    /// engine-driven target move is attributed to that boundary, not to evidence
    /// or a self-correction (the 2026-08-24 review's Codex I11).
    pub prior_target_parameter_version: Option<String>,
    /// The prior audit's split-bridge anchor bar — the exact re-basis factor's
    /// stored leg ([`crate::portfolio::HoldingAudit::authoring_close`]). `None`
    /// on a debut or a prior row from a no-price exit.
    pub prior_authoring_close: Option<crate::portfolio::engine::DatedValue>,
    /// The prior run's pre-profit overlay record (from the audit row) — the
    /// period-keyed observation history accumulates through it
    /// (`docs/portfolio-analysis.md` §Starting parameters). `None` on a debut or a
    /// fund.
    pub prior_pre_profit: Option<crate::portfolio::pre_profit::PreProfitOverlay>,
    /// The loop-time listing-resolution guard's outcome for a stock
    /// (`docs/portfolio-analysis.md` §Asset eligibility) — computed at gather time,
    /// routed by `analyze_holding` beside the eligibility gates. `None` on a fund
    /// (the guard is stocks-only); a stock always carries `Some` — offline stubs
    /// ride the trait default's `Unverified`, which proceeds with a recorded
    /// degraded input, never a terminal outcome.
    pub listing: Option<crate::portfolio::listing::ListingResolution>,
    /// The item-classified 8-K filings sweep's outcome — the hard-forensic
    /// filing kinds' producer state (`docs/portfolio-analysis.md` §Starting
    /// parameters). `None` when the leg never ran (a fund, a skipped retrieval,
    /// or a stub without the source wired); a live stock gather always carries
    /// `Some` — `Unknown` where the sweep couldn't run, never a fabricated clear.
    pub filing_events: Option<crate::portfolio::ForensicFilingState>,
    /// The run-level commodity prints matched to this holding's sector
    /// ([`commodity_prints_for_holding`]) — empty for a non-commodity-linked
    /// holding, a fund, or a run whose commodity leg never ran.
    pub commodity_context: Vec<CommodityPrint>,
    /// This holding's FINRA short-interest row, looked up off the once-per-run
    /// consolidated file (`docs/data-sources.md §FINRA`) — risk /
    /// squeeze-context **positioning evidence**, held out of every sub-score.
    /// `None` on a fund, a symbol absent from the file (a market fact, not a
    /// gap), or a run whose file fetch gapped (that gap rides data health).
    pub short_interest: Option<crate::finra::ShortInterestRead>,
    /// The typed same-underlying option overlay ([`OptionOverlay`]) — the
    /// holding's option legs off the Step-2 pull, classified, with delta and
    /// coverage. `None` on the common no-option-legs case, funds, and skipped
    /// retrievals. The verdict and the action call both see it — the overlay
    /// changes what the right action is.
    pub option_overlay: Option<OptionOverlay>,
    /// The run-level CBOE venue-level put/call backdrop — the same value on
    /// every dossier (broad-market sentiment context, never a per-name signal);
    /// `None` when the leg failed or never ran (its gap rides data health).
    pub put_call_backdrop: Option<crate::cboe::PutCallBackdrop>,
    /// This holding's sector benchmark series ([`BenchmarkSeries`]) — present
    /// on a stock whose sector resolved to a SPDR benchmark and whose run
    /// fetched it; the technology-event pre-flag's read-against leg.
    pub sector_benchmark: Option<BenchmarkSeries>,
    /// The Step-6a semantic continuity recall ([`SemanticRecall`]) — prompt
    /// fragments retrieved from the Portfolio memory partition's `summary` rows,
    /// with the fail-soft gap where the lane failed. Empty-and-gapless on a
    /// debut-empty partition (the first post-slice run, by design), a
    /// not-gradeable holding, or an unconfigured embedder.
    pub semantic_recall: SemanticRecall,
    /// The symbol-scoped `news/stock` headlines fetched at dossier assembly as
    /// research-loop **seeds** — leads, never evidence (`docs/web-research.md`
    /// §The research loop and context management). Empty on a fund, a failed
    /// or skipped fetch (fail-soft), and every offline stub.
    pub news_seeds: Vec<crate::portfolio::research::ResearchSeed>,
    /// The holding's persisted per-topic distilled-findings layer — the
    /// research-reuse priors the loop seeds from and the distillation merges
    /// (`docs/portfolio-analysis.md` §Starting parameters). Empty on a debut
    /// or while no layer exists; expiry is filtered at consumption.
    pub research_priors: Vec<crate::portfolio::research::TopicDistillate>,
    /// The data sources that contributed, for the run's audit record.
    pub sources: Vec<String>,
}

/// The Step-6a semantic continuity retrieval's outcome
/// (`docs/portfolio-workflow.md` §Step 6a): the recalled prompt fragments from
/// this job's own memory partition, or the typed fail-soft gap. A failure skips
/// recall for this holding only — the deterministically loaded prior verdict and
/// ledger are unaffected. The gap is recorded on the audit's degraded inputs at
/// the interpretation paths (never fed to the engine stand-in's degradation
/// count, matching the narrative / pre-flag gap treatment).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemanticRecall {
    pub hits: Vec<String>,
    pub gap: Option<String>,
}

/// The prior run's carry-over for one holding, read off the latest persisted run:
/// the verdict (continuity input) plus the audit-row legs the next pass consumes.
#[derive(Debug, Clone)]
pub struct PriorHolding {
    pub verdict: HoldingVerdict,
    /// The grade-parameter version the prior letter and sub-scores were computed
    /// under (`None` when the run carries no audit row for the symbol).
    pub grade_parameter_version: Option<String>,
    /// The scenario-target parameter version the prior targets were priced under
    /// (`None` when the run carries no audit row for the symbol or the audit
    /// carries no target record).
    pub target_parameter_version: Option<String>,
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
    /// The prior run's NTM consensus-EPS mid (the same stored basis) — the
    /// narrative-vs-reality read's revision comparator. `None` where the prior
    /// basis carried none.
    pub consensus_eps_mid: Option<f64>,
    /// The prior run's matured outcome-window lines for this symbol.
    pub matured_notes: Vec<String>,
    /// The prior run's stored engine metrics (its audit row's `metrics`) — the
    /// metric-level input delta's prior side. `None` without an audit row.
    pub metrics: Option<crate::portfolio::engine::ComputedMetrics>,
    /// The prior audit's split-bridge anchor bar
    /// ([`crate::portfolio::HoldingAudit::authoring_close`]) — re-read from this
    /// run's fresh series it yields the exact re-basis factor since the prior
    /// pass. `None` on a no-price exit's row (those comparisons run as stored).
    pub authoring_close: Option<crate::portfolio::engine::DatedValue>,
}

impl HoldingDossier {
    /// The prior run's thesis ledger for this holding — it rides the prior verdict
    /// (`docs/portfolio-analysis.md` §The position thesis ledger: read at dossier
    /// assembly, re-evaluated and rewritten each run). `None` on a debut or a
    /// not-rated prior.
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
    // Each fill reports whether it wrote. The stamp below reads only the FLOW
    // fills — the annual basis is a flow-window basis, so `Annual` asserts that
    // SEC's full-year flow lines are what the flows stand on; the equity fill is
    // a balance-sheet instant outside the flow-basis rule and stamps nothing
    // (Codex round 2 on the ledger-basis slice: an equity-only SEC fill had still
    // stamped `Annual`, so the prompt called flows that never reached the engine
    // "SEC annual").
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
    // The equity fill stamps its own SOURCE — the balance-sheet instants' second
    // continuity marker (`CompanyFinancials::equity_source`, the 2026-08-24
    // review's Codex I13): FMP's quarterly balance sheet where the FMP leg supplied
    // equity, SEC's annual print where it fell back, `None` where neither did.
    // This is the one seam that knows what finally filled it, so no producer can
    // set the level without recording its source; the stamp alters no value.
    let fmp_supplied_equity = fmp.total_equity.is_some();
    let sec_filled_equity = fill(&mut fmp.total_equity, sec.stockholders_equity);
    fmp.equity_source = if fmp_supplied_equity {
        Some(crate::portfolio::EquitySource::FmpQuarterly)
    } else if sec_filled_equity {
        Some(crate::portfolio::EquitySource::SecAnnual)
    } else {
        None
    };

    // Refine the basis stamp now that the fills have run — see
    // [`apply_ttm_statement_basis`]. An adopted TTM window stands; otherwise the basis
    // is `Annual` exactly when SEC filled a flow line — the annual-derived flows the
    // basis-continuity gate must cover (stamping them `None` would slip them past
    // it) — and `None` when no flow came from SEC: a fund, a holding whose statement
    // surface resolved to nothing, or a balance-sheet instant standing alone (FMP's
    // own beside thin quarters, or an equity-only SEC fill). Those instant-only
    // shapes used to stamp `Annual` ("however it arrived"), which the audit's
    // sources line and the prompt's basis label then read as an SEC annual flow
    // basis for flows that never reached the engine (Codex rounds 1–2 on the
    // ledger-basis slice). Equity-SOURCE continuity — a D/E or P/B step when the
    // equity leg moves between FMP's quarterly instant and SEC's annual one — is
    // not this stamp's (an FMP balance-sheet gap flips the source under an
    // unchanged TTM basis); it rides `equity_source` above, the instants' own.
    if !ttm_statement_basis {
        fmp.statement_basis = sec_filled_a_flow.then_some(crate::portfolio::StatementBasis::Annual);
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

/// One retrieval leg's outcome as the job hands it to assembly, so the audit's
/// source list can tell a leg that was **consulted** (a request was made — the
/// contract at `docs/storage.md` §Local Analysis Suite Storage: "the source labels
/// name every adapter the holding consulted") from one the job never ran. The SEC
/// company-facts leg and the Schwab option-chain leg share this vocabulary.
#[derive(Debug)]
pub enum LegOutcome<'a, T> {
    /// The job never ran this leg (a fund, a guard-terminal stock, a class the
    /// pipeline never grades, a ticker with no CIK mapping). No source label.
    NotRun,
    /// The leg ran and returned nothing (or failed — the failure's gap lands in the
    /// manifest separately). Labeled as consulted, with the empty note.
    Empty,
    /// The leg ran and returned this.
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
    /// The value the leg returned, if any.
    pub fn value(self) -> Option<&'a T> {
        match self {
            LegOutcome::Got(v) => Some(v),
            LegOutcome::NotRun | LegOutcome::Empty => None,
        }
    }
}

/// Assemble the dossier from already-fetched pieces. Pure: the network fetches (FMP,
/// SEC, the Schwab chain) happen in the job, which hands the results here so this
/// assembly stays deterministic and testable. The options signal is computed from the
/// chain when present; absent, it is empty (and the grade is unaffected, since the
/// signal never feeds it).
///
/// `sec_facts` and `chain` each carry their leg's [`LegOutcome`]: `Got` / `Empty`
/// whenever the job **consulted** the leg (a request was made — the SEC facts
/// endpoint queried, the Schwab chain requested), `NotRun` when it never did, so the
/// audit's source list can tell consulted-but-empty from not consulted. A `Got` SEC
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
    semantic_recall: SemanticRecall,
    news_seeds: Vec<crate::portfolio::research::ResearchSeed>,
    research_priors: Vec<crate::portfolio::research::TopicDistillate>,
) -> HoldingDossier {
    let (
        prior_verdict,
        prior_grade_parameter_version,
        prior_target_parameter_version,
        prior_pre_profit,
        prior_vintage,
        prior_spot,
        prior_consensus_eps_mid,
        prior_matured_notes,
        prior_metrics,
        prior_authoring_close,
    ) = match prior {
        Some(p) => (
            Some(p.verdict),
            p.grade_parameter_version,
            p.target_parameter_version,
            p.pre_profit,
            Some(p.vintage),
            p.spot,
            p.consensus_eps_mid,
            p.matured_notes,
            p.metrics,
            p.authoring_close,
        ),
        None => (None, None, None, None, None, None, None, Vec::new(), None, None),
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
    // surface — its audit must not claim it (`docs/portfolio-analysis.md` §Asset
    // eligibility). Two reasons skip it, and the audit names the right one: a
    // guard-terminal stock, whose profile identity read is the evidence that actually
    // drove the verdict; and a class the equity pipeline never grades, where the
    // eligibility routing decided the verdict before any request.
    let guard_terminal = matches!(
        &listing,
        Some(
            crate::portfolio::listing::ListingResolution::Unresolved
                | crate::portfolio::listing::ListingResolution::NonUs { .. }
                | crate::portfolio::listing::ListingResolution::Conflict { .. }
        )
    );
    //
    // Every holding's position values — eligibility, side, P/L, the action call's
    // sizing evidence — come from the Schwab holdings snapshot, so every branch names
    // it: the not-gradeable branch in its own wording (there the snapshot's asset
    // class is the evidence that decided the verdict), the guard-terminal branch
    // beside the profile read that decided it, and the gradeable branch beside the
    // FMP pull that actually ran — the stock statements / consensus surface, or the
    // fund's quote + EOD + dividend surface (`FmpDataSource::fetch_fund_financials`),
    // which never touches the statement endpoints.
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
    // The one-per-stock profile lookup runs whenever a listing resolution is
    // present, and on the common (resolved / unverified) route it still supplied the
    // listing identity, the issuer name, and the outcome sector — so it is a
    // consulted source there too, not only when it was guard-terminal.
    if listing.is_some() && !guard_terminal {
        sources.push("FMP company profile (listing identity, issuer name, sector)".to_string());
    }
    // The adopted statement basis, on either basis — read off the MERGED
    // financials, since the SEC merge is what settles an annual fallback
    // (`merge_financials`); `None` (no adopted flow basis) records nothing
    // (`docs/portfolio-analysis.md` §Starting parameters).
    match financials.statement_basis {
        Some(crate::portfolio::StatementBasis::Ttm) => {
            sources.push("FMP TTM statement basis (four-quarter sums)".to_string())
        }
        Some(crate::portfolio::StatementBasis::Annual) => {
            sources.push("SEC annual statement basis (latest full-year lines)".to_string())
        }
        None => {}
    }
    // SEC is labeled whenever the leg was consulted — a fetch that returned nothing
    // (or failed, with its gap in the manifest) still leaves its trace, distinct from
    // a leg the job never ran (a fund, a skipped retrieval, or a ticker with no CIK
    // mapping — where the facts endpoint was never queried and the gap says so).
    match sec_facts {
        LegOutcome::Got(facts) if !facts.is_empty() => {
            sources.push("SEC EDGAR company facts".to_string());
        }
        LegOutcome::Got(_) | LegOutcome::Empty => {
            sources.push("SEC EDGAR company facts (empty)".to_string());
        }
        LegOutcome::NotRun => {}
    }
    // The chain likewise: every eligible stock requests it, so a request that came
    // back with no chain (or failed, its gap in the manifest) is still a consulted
    // adapter, distinct from a leg the retrieval gate skipped.
    match chain {
        LegOutcome::Got(_) => sources.push("Schwab option chain".to_string()),
        LegOutcome::Empty => sources.push("Schwab option chain (none returned)".to_string()),
        LegOutcome::NotRun => {}
    }
    // The filings sweep labels only where its endpoint was actually queried: a
    // classified or clean sweep, or a queried-but-failed one ("unavailable").
    // An unqueried `Unknown` (no CIK mapping) and a leg that never ran leave no
    // label — the gap manifest carries the reason.
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
    // The same-underlying option overlay: its positions ride the already-labeled
    // Schwab snapshot, and the targeted delta fetch labels only where it actually
    // served or failed (the stub default never makes a request).
    if let Some(o) = &option_overlay {
        sources.push("Schwab option positions (same-underlying overlay)".to_string());
        if o.delta_source_consulted {
            sources.push("Schwab option chain (overlay-delta strikes)".to_string());
        }
    }
    // The run-level commodity context deliberately does NOT label here: like
    // the house view, whether a verdict actually consulted it is unknowable at
    // assembly (the early exits never render a prompt), so the interpretation
    // paths — the only readers — add the label themselves in `pipeline`.
    if fund.is_some() {
        sources.push(
            "FMP fund metadata (etf/info + profile structure read + weightings + sector P/E)"
                .to_string(),
        );
    }
    // The house view is deliberately **not** listed here, even though it is loaded once
    // per run and rides every dossier: whether a holding's verdict actually consulted
    // it is not knowable at assembly. Many routes through
    // `pipeline::analyze_holding` return before either 6f prompt — the eligibility
    // gate, the listing guard, a net-short or fully-offset position, and every
    // evidence-floor abstention — and listing it here made all of their audits claim a
    // source the verdict never read.
    //
    // Enumerating those exits in the dossier is the shape that keeps going wrong: it
    // has to be re-derived whenever a new exit lands. So the default is the honest one
    // — absent — and the interpretation paths, which are the only ones that read it,
    // add it themselves ([`HOUSE_VIEW_SOURCE`], appended in `pipeline`).

    HoldingDossier {
        position,
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
        prior_consensus_eps_mid,
        prior_matured_notes,
        prior_metrics,
        prior_grade_parameter_version,
        prior_target_parameter_version,
        prior_authoring_close,
        prior_pre_profit,
        listing,
        filing_events,
        short_interest,
        option_overlay,
        put_call_backdrop,
        commodity_context,
        sector_benchmark,
        semantic_recall,
        news_seeds,
        research_priors,
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
/// verdict plus the audit-row legs — the grade-parameter version its letter and
/// sub-scores were computed under (`None` when the run carries no audit row) and the
/// pre-profit overlay record whose observation history accumulates. Reads the
/// job's **already-loaded** prior run — the job loads `store::latest_run` once
/// per run and threads it here, rather than this lookup re-reading (and
/// re-parsing) the store once per holding — and finds the matching symbol;
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
    // selective carry re-persists an old verdict (and its audit's authoring-spot
    // basis) into a newer run, so dating the retrospective off the container
    // would pair run A's spot with run B's date (Codex round 2, finding 2).
    let vintage = crate::portfolio::effective_vintage(&verdict, &run.created_at).to_string();
    let audit_row = run
        .audit
        .iter()
        .find(|a| a.symbol.eq_ignore_ascii_case(symbol));
    let (
        grade_parameter_version,
        target_parameter_version,
        pre_profit,
        spot,
        consensus_eps_mid,
        metrics,
        authoring_close,
    ) = match audit_row {
        Some(a) => {
            let spot = a.quick_basis.as_ref().map(|b| b.spot);
            let mid = a.quick_basis.as_ref().and_then(|b| b.consensus_eps_mid);
            (
                Some(a.grade_parameter_version.clone()),
                // The target stamp rides the typed target record, so a prior with
                // no target record (never priced) carries none — silent downstream.
                a.target_meta.as_ref().map(|t| t.parameter_version.clone()),
                a.pre_profit.clone(),
                spot,
                mid,
                Some(a.metrics.clone()),
                a.authoring_close.clone(),
            )
        }
        None => (None, None, None, None, None, None, None),
    };
    // The prior run's matured outcome lines for this symbol — the deterministic
    // scored ground the retrospective block renders (empty until windows mature).
    let matured_notes = run
        .outcome
        .matured
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
        .collect();
    Some(PriorHolding {
        verdict,
        grade_parameter_version,
        target_parameter_version,
        pre_profit,
        vintage,
        spot,
        consensus_eps_mid,
        matured_notes,
        metrics,
        authoring_close,
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
        // Energy → the energy sleeve alone.
        assert_eq!(labels(Some("Energy"), None), vec!["WTI Crude Oil".to_string()]);
        // Basic Materials → the metals sleeve; gold only on a gold-linked
        // industry — a steel or chemicals holding carries no gold evidence
        // (Codex 2026-08-20, finding 4).
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
        // Any other sector — or none — carries no commodity block.
        assert!(labels(Some("Technology"), None).is_empty());
        assert!(labels(None, None).is_empty());
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

    /// Codex I13: the merge stamps which balance sheet supplied the equity — the
    /// instants' own continuity marker — on either flow basis, and `None` where
    /// no leg did.
    #[test]
    fn merge_stamps_which_balance_sheet_supplied_the_equity() {
        use crate::portfolio::EquitySource;
        let sec = CompanyFacts {
            stockholders_equity: Some(60_000_000_000),
            ..Default::default()
        };
        // FMP's quarterly balance sheet supplied equity: FMP's, SEC's print unread.
        let mut fmp = fmp_only();
        fmp.total_equity = Some(50_000_000_000.0);
        let merged = merge_financials(fmp, &sec, true);
        assert_eq!(merged.total_equity, Some(50_000_000_000.0));
        assert_eq!(merged.equity_source, Some(EquitySource::FmpQuarterly));
        // The FMP leg gapped: SEC's annual print fills and the stamp says so — on
        // either flow basis, since the instant sits outside the flow-basis rule.
        for ttm in [true, false] {
            let merged = merge_financials(fmp_only(), &sec, ttm);
            assert_eq!(merged.total_equity, Some(60_000_000_000.0));
            assert_eq!(
                merged.equity_source,
                Some(EquitySource::SecAnnual),
                "ttm={ttm}"
            );
        }
        // Neither leg: no equity, no source.
        let merged = merge_financials(fmp_only(), &CompanyFacts::default(), false);
        assert_eq!(merged.total_equity, None);
        assert_eq!(merged.equity_source, None);
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
    fn an_equity_instant_alone_carries_no_flow_basis_and_a_sec_flow_fill_is_annual() {
        use crate::portfolio::StatementBasis;
        // Codex round 1 on the ledger-basis slice: thin quarters beside FMP's own
        // balance sheet, with the SEC leg never run (no CIK), used to stamp
        // `Annual` off the equity instant alone — SEC provenance for a level SEC
        // never supplied. An instant on no flow basis is `None`.
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
        // Codex round 2: SEC supplying only equity (an issuer with no matching
        // revenue concept) fills an instant, not a flow — the annual FLOW basis is
        // not stamped, or the prompt would call flows that never reached the
        // engine "SEC annual".
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
            SemanticRecall::default(),
            Vec::new(),
            Vec::new(),
        );
        assert!(dossier.sources.iter().any(|s| s.contains("FMP")));
        assert!(dossier.sources.iter().any(|s| s.contains("SEC")));
        assert!(dossier.sources.contains(&"Schwab option chain".to_string()));
        assert!(dossier.options_signal.put_call_volume.unwrap() > 1.0);
        assert!(dossier.prior_verdict.is_none(), "new holding");
    }

    #[test]
    fn option_chain_leg_is_labeled_whenever_consulted_and_none_is_distinct_from_not_run() {
        // Every eligible stock requests the chain (`job`), so a request that came back
        // with nothing — or failed, its gap in the manifest — is still a consulted
        // adapter (`docs/storage.md`: "the source labels name every adapter the
        // holding consulted"); only a leg the retrieval gate skipped leaves no label.
        let chain_labels = |sources: Vec<String>| -> Vec<String> {
            sources.into_iter().filter(|s| s.contains("option chain")).collect()
        };
        let chain = OptionChain {
            underlying: "AAPL".into(),
            underlying_price: Some(195.0),
            contracts: vec![],
        };
        // Consulted, returned a chain: the plain label.
        assert_eq!(
            chain_labels(stock_sources_with_chain(LegOutcome::Got(&chain))),
            vec!["Schwab option chain".to_string()]
        );
        // Consulted, none returned (or the request failed): labeled as consulted,
        // with the none-returned note.
        assert_eq!(
            chain_labels(stock_sources_with_chain(LegOutcome::Empty)),
            vec!["Schwab option chain (none returned)".to_string()]
        );
        // Never requested: no chain label at all.
        assert!(chain_labels(stock_sources_with_chain(LegOutcome::NotRun)).is_empty());
    }

    #[test]
    fn a_gradeable_holding_names_the_schwab_position_snapshot() {
        // Every holding's position values — eligibility, side, P/L, the action call's
        // sizing evidence — come from the Schwab holdings snapshot, so a gradeable
        // holding names it beside the FMP pull; the not-gradeable branch keeps its
        // own wording (pinned in `assembly_never_claims_the_house_view_...`).
        let sources = stock_sources(LegOutcome::NotRun, None);
        assert_eq!(
            sources,
            vec![
                "Schwab position (holdings snapshot)".to_string(),
                "FMP company financials".to_string(),
            ],
            "{sources:?}"
        );
        // A fund's FMP pull is `fetch_fund_financials` — quote + EOD + dividends, never
        // the statement / consensus endpoints — so its label says so rather than
        // borrowing the stock surface's name (Codex round 2, 2026-08-18).
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
            SemanticRecall::default(),
            Vec::new(),
            Vec::new(),
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
        // The house view is loaded once per run and rides EVERY dossier, so listing it
        // unconditionally made every ordinary cash, option and bond audit claim a
        // source that holding's verdict never consulted: both the non-gradeable
        // eligibility route and the guard-terminal route return from
        // `pipeline::analyze_holding` ahead of either 6f prompt and the per-holding
        // action call's prompt.
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
                SemanticRecall::default(),
                Vec::new(),
                Vec::new(),
            )
            .sources
        };

        // A class the equity pipeline never grades: only the evidence that decided it.
        assert_eq!(
            assemble_with(AssetClass::Cash),
            vec!["Schwab position (cash — not graded by the equity pipeline)".to_string()],
        );

        // And the same for a GRADEABLE holding: assembly cannot know whether the
        // verdict will reach an interpretation call, so it claims nothing either way.
        // The interpretation paths add it themselves — pinned in `pipeline`, both
        // directions.
        assert!(
            !assemble_with(AssetClass::Stock)
                .iter()
                .any(|src| src.contains("house view")),
            "assembly never claims the house view — it cannot know"
        );
    }

    /// The source list for a stock assembled with the given SEC leg, chain leg, and
    /// listing, over the FMP-only financials.
    fn stock_sources_full(
        sec_facts: LegOutcome<'_, CompanyFacts>,
        chain: LegOutcome<'_, OptionChain>,
        listing: Option<crate::portfolio::listing::ListingResolution>,
    ) -> Vec<String> {
        stock_sources_assembled(fmp_only(), sec_facts, chain, listing)
    }

    /// The source list for a stock assembled over the given FMP financials, SEC
    /// leg, chain leg, and listing.
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
            SemanticRecall::default(),
            Vec::new(),
            Vec::new(),
        )
        .sources
    }

    /// The source list for a stock assembled with the given SEC leg and listing (no
    /// chain leg run).
    fn stock_sources(
        sec_facts: LegOutcome<'_, CompanyFacts>,
        listing: Option<crate::portfolio::listing::ListingResolution>,
    ) -> Vec<String> {
        stock_sources_full(sec_facts, LegOutcome::NotRun, listing)
    }

    /// The source list for a stock assembled with the given chain leg (no SEC leg
    /// run, no listing).
    fn stock_sources_with_chain(chain: LegOutcome<'_, OptionChain>) -> Vec<String> {
        stock_sources_full(LegOutcome::NotRun, chain, None)
    }

    /// The 2026-08-24 large-scale review's Priority-1 minor (the ledger TTM
    /// vocabulary slice, folded): the sources line recorded the basis only when
    /// TTM was adopted, so an annual-fallback holding's audit named no basis while
    /// `portfolio-analysis.md` §Starting parameters says the adopted basis is
    /// recorded there.
    #[test]
    fn sources_line_names_the_adopted_statement_basis() {
        let basis_labels = |sources: Vec<String>| -> Vec<String> {
            sources
                .into_iter()
                .filter(|s| s.contains("statement basis"))
                .collect()
        };
        // Four contiguous quarters: the TTM basis, and only it.
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
        // No usable quarters, SEC annual facts filling the levels: the annual basis.
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
        // FMP's own balance sheet beside thin quarters, SEC never run: no basis, so
        // no label claims SEC provenance (Codex round 1).
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
        // An equity-only SEC fill is the same instant-only shape: no flow basis, no
        // label (Codex round 2).
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
        // No statement lines from anywhere: no basis, and no label claiming one.
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
        // M3 of the 2026-08-18 doc/code audit: the label used to depend on the facts
        // being nonempty, so a consulted-but-empty EDGAR fetch left no trace and read
        // exactly like a leg the job never ran.
        let sec_labels = |sources: Vec<String>| -> Vec<String> {
            sources
                .into_iter()
                .filter(|s| s.contains("SEC EDGAR"))
                .collect()
        };
        // Consulted, nonempty: the plain label.
        let facts = CompanyFacts {
            revenue: Some(400_000_000_000),
            ..CompanyFacts::default()
        };
        assert_eq!(
            sec_labels(stock_sources(LegOutcome::Got(&facts), None)),
            vec!["SEC EDGAR company facts".to_string()]
        );
        // Consulted, empty: labeled as consulted, with the empty note — whether the
        // job hands over the empty facts a queried endpoint returned or the bare
        // `Empty` outcome.
        assert_eq!(
            sec_labels(stock_sources(LegOutcome::Got(&CompanyFacts::default()), None)),
            vec!["SEC EDGAR company facts (empty)".to_string()]
        );
        assert_eq!(
            sec_labels(stock_sources(LegOutcome::Empty, None)),
            vec!["SEC EDGAR company facts (empty)".to_string()]
        );
        // Never run: no SEC label at all. This is also the no-CIK case — the facts
        // endpoint was never queried, and the "no CIK mapping" gap (recorded by the
        // job's `sec_company_facts`, pinned there) is the trace it leaves.
        assert!(sec_labels(stock_sources(LegOutcome::NotRun, None)).is_empty());
    }

    #[test]
    fn profile_lookup_is_a_source_on_the_common_resolved_route_too() {
        // The one-per-stock profile lookup feeds the listing identity, the issuer
        // name, and the outcome sector on every stock — the audit named it only when
        // it was guard-terminal (M3, 2026-08-18).
        use crate::portfolio::listing::ListingResolution;
        let identity = "FMP company profile (listing identity, issuer name, sector)".to_string();
        let guard = "FMP company profile (listing-resolution guard)".to_string();

        // Resolved: the identity label beside the financials pull.
        let resolved = stock_sources(LegOutcome::NotRun, Some(ListingResolution::SupportedUs));
        assert!(resolved.contains(&identity), "{resolved:?}");
        assert!(resolved.contains(&"FMP company financials".to_string()));
        assert!(!resolved.contains(&guard));
        // Unverified (an FMP profile gap): the lookup still ran and is named; the
        // guard's degraded input is the pipeline's to record.
        let unverified = stock_sources(
            LegOutcome::NotRun,
            Some(ListingResolution::Unverified {
                detail: "FMP profile unavailable".into(),
            }),
        );
        assert!(unverified.contains(&identity), "{unverified:?}");
        // Guard-terminal: the Schwab snapshot the position came from plus the guard
        // label — the profile read is the evidence that decided the verdict, and no
        // FMP financials pull ran.
        let terminal = stock_sources(LegOutcome::NotRun, Some(ListingResolution::Unresolved));
        assert_eq!(
            terminal,
            vec!["Schwab position (holdings snapshot)".to_string(), guard]
        );
        // No listing resolution (no lookup ran): no profile label.
        assert!(
            !stock_sources(LegOutcome::NotRun, None).iter().any(|s| s.contains("company profile")),
            "{:?}",
            stock_sources(LegOutcome::NotRun, None)
        );
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
        // No prior run -> no prior verdict.
        assert!(prior_verdict_for(None, "AAPL").is_none());

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
                side_reversed: false,
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: Default::default(),
                overview: String::new(),
            },
            audit: vec![],
            rate_prints: Default::default(),
            outcome: Default::default(),
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let latest = crate::portfolio::store::latest_run(&conn).unwrap();
        let prior = prior_verdict_for(latest.as_ref(), "aapl").expect("case-insensitive match");
        assert_eq!(prior.verdict.symbol, "AAPL");
        // No audit row for the symbol -> no stamp to read, on either axis.
        assert_eq!(prior.grade_parameter_version, None);
        assert_eq!(prior.target_parameter_version, None);
        assert!(prior.pre_profit.is_none());
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
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                thesis_ledger: None,
                analyzed_at: None,
                action_source: Default::default(),
                side_reversed: false,
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: Default::default(),
                overview: String::new(),
            },
            audit: vec![crate::portfolio::HoldingAudit {
                what_changed_audit: None,
                research: None,
                symbol: "AAPL".into(),
                metrics: Default::default(),
                sources: vec![],
                model_ids: vec![],
                prompt_version: crate::portfolio::PROMPT_VERSION.to_string(),
                evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.to_string(),
                degraded_inputs: vec![],
                action_annotations: vec![],
                // The target stamp rides the typed target record (Codex I11).
                target_meta: Some(crate::portfolio::engine::TargetMeta {
                    parameter_version: "targets-v4".into(),
                    ..Default::default()
                }),
                grade_parameter_version: "grade-v2".into(),
                ledger_audit: None,
                quick_basis: None,
                authoring_close: None,
                fund_exposure: None,
                pre_profit: None,
                hurdle: None,
                forensic: None,
                tech_event_pre_flag: None,
                short_interest: None,
                implied_expectations: None,
                narrative: None,
                option_overlay: None,
            }],
            rate_prints: Default::default(),
            outcome: Default::default(),
        };
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let latest = crate::portfolio::store::latest_run(&conn).unwrap();
        let prior = prior_verdict_for(latest.as_ref(), "AAPL").expect("verdict present");
        assert_eq!(prior.grade_parameter_version.as_deref(), Some("grade-v2"));
        assert_eq!(prior.target_parameter_version.as_deref(), Some("targets-v4"));
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
                side_reversed: false,
            }],
            roll_up: crate::portfolio::PortfolioRollUp {
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                role_risk_only_count: 0,
                top_position_weight: 0.0,
                cash_weight: 0.0,
                exited: vec![],
                data_health: Default::default(),
                overview: String::new(),
            },
            audit: vec![],
            rate_prints: Default::default(),
            outcome: Default::default(),
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
        // No option rows → no overlay at all — and a zero-net row (fully
        // offset contracts) is no exposure, never a leg or an overlay.
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
        // A naked short call (2 contracts over 100 shares) must never read
        // covered — `other`, with the naked note.
        let rows = [opt_row(CALL_210, -2.0)];
        let refs: Vec<&Position> = rows.iter().collect();
        let o = assemble_option_overlay(100.0, &refs, no_delta, false).unwrap();
        assert_eq!(o.class, OverlayClass::Other, "{o:?}");
        assert!(o.coverage_ratio.is_none());
        assert!(o.gaps.iter().any(|g| g.contains("naked")), "{o:?}");
        // An unrecognized symbol forces `other`; its delta is a gap, so the
        // net delta is None — whole or not at all.
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
