//! SEC EDGAR — a keyless primary source for company financials
//! (`docs/data-sources.md §SEC EDGAR`), used by the local Portfolio Analysis job
//! alongside FMP. This slice reads the **XBRL company-facts** API
//! (`/api/xbrl/companyfacts/CIK##########.json`), pulling the latest annual values
//! for a handful of GAAP concepts so the financial-analysis engine can cross-check
//! and fill gaps the FMP per-company pull leaves.
//!
//! Like the gated adapters it carries a base-URL injection seam so a localhost mock
//! exercises the full URL-build → retry → parse → domain-output wire path offline
//! (`crate::test_http`). It is **keyless** (like BLS/CFTC) — the only requirement is
//! a descriptive `User-Agent`, which SEC asks all automated clients to send. Failures
//! are fail-soft: a concept that can't be resolved is a `None`, not a fabricated
//! level, mirroring the data-honesty stance of every other adapter.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::progress::RunContext;

/// SEC EDGAR data host. The company-facts path is joined onto this.
const SEC_DATA_BASE: &str = "https://data.sec.gov";

/// SEC asks automated clients to identify themselves with a descriptive User-Agent
/// (a generic browser UA gets throttled). Static, since this is an app-level client.
const SEC_USER_AGENT: &str = "MarketSignal local-analysis (support@market-signal.app)";

/// The company-facts endpoint path; `{cik}` is the 10-digit zero-padded CIK.
fn company_facts_path(cik10: &str) -> String {
    format!("/api/xbrl/companyfacts/CIK{cik10}.json")
}

/// The submissions endpoint serving a filer's recent-filings index — the quick
/// check's EDGAR filing sweep (`docs/portfolio-analysis.md` §The quick check).
fn submissions_path(cik10: &str) -> String {
    format!("/submissions/CIK{cik10}.json")
}

/// The latest annual values pulled from a company's XBRL facts — each `None` when the
/// concept was not reported (or could not be resolved). Deliberately a small set: the
/// lines the engine cross-checks against FMP.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompanyFacts {
    pub revenue: Option<i64>,
    /// The prior fiscal year's revenue — read from the **same concept** that
    /// supplied `revenue` (never a different tag, so the growth read can't mix
    /// bases), the second-latest distinct annual period end. Feeds the annual-basis
    /// `revenue_growth` fallback where the FMP quarterly prints are too thin for
    /// the TTM basis (the grade-band slice's F5 closure).
    pub revenue_prior: Option<i64>,
    pub gross_profit: Option<i64>,
    pub net_income: Option<i64>,
    pub total_assets: Option<i64>,
    pub stockholders_equity: Option<i64>,
}

impl CompanyFacts {
    /// Whether any fact resolved — the dossier uses this to decide if SEC contributed.
    pub fn is_empty(&self) -> bool {
        self.revenue.is_none()
            && self.revenue_prior.is_none()
            && self.gross_profit.is_none()
            && self.net_income.is_none()
            && self.total_assets.is_none()
            && self.stockholders_equity.is_none()
    }
}

/// Where the full ticker → CIK map lives. It is served from the `www.sec.gov` host,
/// not the `data.sec.gov` API host the company-facts call uses, so it carries its own
/// base-URL seam.
const SEC_TICKERS_BASE: &str = "https://www.sec.gov";

/// The company-tickers file path on [`SEC_TICKERS_BASE`].
const SEC_TICKERS_PATH: &str = "/files/company_tickers.json";

/// How long a cached `company_tickers.json` stays fresh before a run refreshes it
/// (drafted — CIK assignments change rarely, so a week keeps the map current without
/// re-downloading the ~1 MB file per run). A stale cache is still used when the
/// refresh fetch fails: fail-soft, never a run blocker.
pub const CIK_CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The ticker → CIK resolver over SEC's full `company_tickers.json` map
/// (`docs/data-sources.md §SEC EDGAR`). Resolution returns the 10-digit zero-padded
/// CIK EDGAR expects; an unresolved ticker stays `None` and degrades to a typed gap
/// at the caller, never a fabricated mapping.
#[derive(Debug, Clone, Default)]
pub struct CikResolver {
    map: std::collections::HashMap<String, String>,
}

impl CikResolver {
    /// An empty resolver — every lookup misses. The fail-soft floor when neither a
    /// cache nor a fetch is available.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Parse the `company_tickers.json` body: an object keyed by row index, each row
    /// `{cik_str, ticker, title}`. The CIK is zero-padded to the 10 digits EDGAR paths
    /// expect.
    pub fn from_json(body: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(body).context("parsing company_tickers.json")?;
        let rows = value
            .as_object()
            .context("company_tickers.json: expected a top-level object")?;
        let mut map = std::collections::HashMap::with_capacity(rows.len());
        for row in rows.values() {
            let (Some(ticker), Some(cik)) = (
                row.get("ticker").and_then(Value::as_str),
                row.get("cik_str").and_then(Value::as_u64),
            ) else {
                continue; // A malformed row is skipped, never a fabricated mapping.
            };
            map.insert(ticker.to_ascii_uppercase(), format!("{cik:010}"));
        }
        Ok(Self { map })
    }

    /// The 10-digit zero-padded CIK for a ticker (case-insensitive), or `None` when
    /// the symbol has no EDGAR mapping.
    pub fn resolve(&self, ticker: &str) -> Option<&str> {
        self.map.get(&ticker.to_ascii_uppercase()).map(String::as_str)
    }

    /// How many tickers resolve — zero means the resolver is the empty fail-soft floor.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// The file name of the cached `company_tickers.json`, kept beside the app database.
const CIK_CACHE_FILE: &str = "sec_company_tickers.json";

/// Where the ticker → CIK cache lives for a given app database: the sibling
/// [`CIK_CACHE_FILE`] in the database's directory (the bare file name when the
/// path has no parent). Single-homed so both local jobs resolve against one cache.
pub fn cik_cache_path_beside(db_path: &std::path::Path) -> std::path::PathBuf {
    db_path
        .parent()
        .map(|d| d.join(CIK_CACHE_FILE))
        .unwrap_or_else(|| std::path::PathBuf::from(CIK_CACHE_FILE))
}

/// Load the ticker → CIK resolver: the on-disk cache when fresh
/// ([`CIK_CACHE_MAX_AGE`]), else a fetch that rewrites the cache. Fail-soft at every
/// step — a failed fetch falls back to a stale cache when one exists, and to the
/// empty resolver when none does, so an SEC outage degrades filings coverage to
/// typed gaps rather than blocking the run.
///
/// The refresh fetch honors the shared cancel flag like every SEC request
/// ([`SecEdgarSource::fetch_company_tickers`] bails without a request when it is
/// set), so it falls to the same stale-or-empty floor. That flag is only cleared
/// once a job owns the global run slot (`RunContext::reset_cancel`), which is why
/// the live jobs defer this load to first use inside the slot
/// ([`LazyCikResolver`]) rather than calling it eagerly at setup: an eager load
/// after a cancelled run would silently ship a stale or empty map into the whole
/// run, and its request row would fire before `run_started`.
pub fn load_cik_resolver(cache_path: &std::path::Path, source: &SecEdgarSource) -> CikResolver {
    let cached = std::fs::read_to_string(cache_path).ok();
    let cache_fresh = std::fs::metadata(cache_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|age| age < CIK_CACHE_MAX_AGE)
        .unwrap_or(false);
    if cache_fresh {
        if let Some(body) = &cached {
            if let Ok(resolver) = CikResolver::from_json(body) {
                return resolver;
            }
        }
    }
    match source.fetch_company_tickers() {
        Ok(body) => match CikResolver::from_json(&body) {
            Ok(resolver) => {
                // Best-effort cache write: a failed write costs the next run a
                // re-download, never this run's resolution.
                if let Some(dir) = cache_path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(cache_path, &body);
                resolver
            }
            Err(_) => stale_or_empty(cached),
        },
        Err(_) => stale_or_empty(cached),
    }
}

/// The fail-soft floor for [`load_cik_resolver`]: a parseable stale cache, else empty.
fn stale_or_empty(cached: Option<String>) -> CikResolver {
    cached
        .and_then(|body| CikResolver::from_json(&body).ok())
        .unwrap_or_else(CikResolver::empty)
}

/// The ticker → CIK resolver **deferred to first use** — the carrier of the local
/// jobs' ordering invariant: every external fetch happens inside the global run
/// slot, after `try_begin` + `reset_cancel` + `run_started`, so the ticker-map
/// refresh (a) sees the run's own cancel state rather than a prior cancelled run's
/// leftover flag, and (b) streams its request row under the active step instead
/// of firing before the tracker is listening. Constructing one performs no I/O;
/// the first [`Self::get`] runs [`load_cik_resolver`] once and memoizes the result
/// for the run (a fail-soft stale/empty map is memoized too — the same one-load-
/// per-run behavior the eager call had). The daemon probe stays the one pre-slot
/// check, and it is local-only.
pub struct LazyCikResolver {
    cache_path: std::path::PathBuf,
    resolver: std::sync::OnceLock<CikResolver>,
}

impl LazyCikResolver {
    /// Bind the cache location; nothing is read or fetched until [`Self::get`].
    pub fn new(cache_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            cache_path: cache_path.into(),
            resolver: std::sync::OnceLock::new(),
        }
    }

    /// A resolver that is already resolved — no cache, no fetch, ever. For a
    /// caller that holds a map (tests, offline smokes).
    pub fn preloaded(resolver: CikResolver) -> Self {
        let cell = std::sync::OnceLock::new();
        let _ = cell.set(resolver);
        Self {
            cache_path: std::path::PathBuf::from(CIK_CACHE_FILE),
            resolver: cell,
        }
    }

    /// The resolver, loading it through `source` on the first call (see
    /// [`load_cik_resolver`]) and serving the memoized map after that.
    pub fn get(&self, source: &SecEdgarSource) -> &CikResolver {
        self.resolver
            .get_or_init(|| load_cik_resolver(&self.cache_path, source))
    }

    /// Resolve one ticker, loading the map on first use — [`CikResolver::resolve`]
    /// behind the lazy load.
    pub fn resolve(&self, source: &SecEdgarSource, ticker: &str) -> Option<&str> {
        self.get(source).resolve(ticker)
    }

    /// Whether the map has been loaded yet — the ordering tests' probe.
    #[cfg(test)]
    pub fn is_loaded(&self) -> bool {
        self.resolver.get().is_some()
    }
}

/// The keyless SEC EDGAR company-facts adapter. Mirrors the gated adapters' shape
/// (`http` + `base_url` + `progress`), minus the API key.
pub struct SecEdgarSource {
    http: reqwest::blocking::Client,
    base_url: String,
    /// The `www.sec.gov` host serving `company_tickers.json` — a distinct base from
    /// the `data.sec.gov` API host, with its own test seam.
    tickers_base_url: String,
    progress: Arc<RunContext>,
}

impl SecEdgarSource {
    /// Build the adapter. The User-Agent SEC asks for is set on the client once.
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(SEC_USER_AGENT)
            .build()
            .context("building the SEC EDGAR HTTP client")?;
        Ok(Self {
            http,
            base_url: SEC_DATA_BASE.to_string(),
            tickers_base_url: SEC_TICKERS_BASE.to_string(),
            progress: RunContext::noop(),
        })
    }

    /// Point the adapter at a mock base URL for the offline round-trip test. Trailing
    /// slash trimmed so the joined path's leading slash doesn't double up. Points both
    /// hosts at the mock, since a test exercises one endpoint at a time (crate-visible
    /// so the portfolio job's slot-ordering test can drive the real fetch path).
    #[cfg(test)]
    pub(crate) fn with_base_url(mut self, base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/').to_string();
        self.tickers_base_url = base.clone();
        self.base_url = base;
        self
    }

    /// Fetch the raw `company_tickers.json` body (the caller parses and caches it —
    /// [`load_cik_resolver`]). A transport error or non-2xx returns `Err`; resolution
    /// then falls back fail-soft.
    pub fn fetch_company_tickers(&self) -> Result<String> {
        if self.progress.is_cancelled() {
            anyhow::bail!("SEC ticker-map fetch skipped (run cancelled)");
        }
        let url = format!("{}{SEC_TICKERS_PATH}", self.tickers_base_url);
        self.progress
            .request_started("SEC", "company-tickers", "all", "SEC ticker→CIK map");
        let result = (|| -> Result<String> {
            let (status, body) =
                crate::http_retry::send_with_retry("SEC", || self.http.get(&url))?;
            if !(200..300).contains(&status) {
                anyhow::bail!("SEC returned {status} for company_tickers.json");
            }
            Ok(body)
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "SEC",
                "company-tickers",
                "all",
                "SEC ticker→CIK map",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "SEC",
                "company-tickers",
                "all",
                "SEC ticker→CIK map",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }

    /// Attach a live run context so each fetch streams a tracker row.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Fetch the company-facts JSON for a CIK and shape it into [`CompanyFacts`]. A
    /// transport error or non-2xx returns `Err`; the caller (the dossier) treats that
    /// fail-soft, since SEC supplements FMP rather than gating the run.
    pub fn fetch_company_facts(&self, cik10: &str) -> Result<CompanyFacts> {
        // Cancel checkpoint before the request: a cancel already requested skips the
        // network (no request, so no tracker row) and surfaces as an error the job's
        // cancel path classifies as a user stop.
        if self.progress.is_cancelled() {
            anyhow::bail!("SEC fetch skipped (run cancelled)");
        }
        let path = company_facts_path(cik10);
        let url = format!("{}{path}", self.base_url);
        self.progress
            .request_started("SEC", "company-facts", cik10, "SEC company facts");
        let result = (|| -> Result<CompanyFacts> {
            let (status, body) =
                crate::http_retry::send_with_retry("SEC", || self.http.get(&url))?;
            if !(200..300).contains(&status) {
                anyhow::bail!("SEC EDGAR returned {status}");
            }
            let value: Value = serde_json::from_str(&body).context("parsing SEC company facts")?;
            Ok(facts_from_value(&value))
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "SEC",
                "company-facts",
                cik10,
                "SEC company facts",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "SEC",
                "company-facts",
                cik10,
                "SEC company facts",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }

    /// A filer's recent filings from the submissions index, newest first — the quick
    /// check's per-stock EDGAR filing sweep. `Err` on transport / non-2xx / parse
    /// failure so the caller types the filing family `unknown` rather than reading a
    /// failed sweep as no-new-filings.
    pub fn fetch_recent_filings(&self, cik10: &str) -> Result<Vec<RecentFiling>> {
        if self.progress.is_cancelled() {
            anyhow::bail!("SEC submissions fetch skipped (run cancelled)");
        }
        let url = format!("{}{}", self.base_url, submissions_path(cik10));
        self.progress
            .request_started("SEC", "submissions", cik10, "SEC recent filings");
        let result = (|| -> Result<Vec<RecentFiling>> {
            let (status, body) =
                crate::http_retry::send_with_retry("SEC", || self.http.get(&url))?;
            if !(200..300).contains(&status) {
                anyhow::bail!("SEC submissions returned {status}");
            }
            let value: Value =
                serde_json::from_str(&body).context("parsing SEC submissions")?;
            recent_filings_from_value(&value)
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "SEC",
                "submissions",
                cik10,
                "SEC recent filings",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "SEC",
                "submissions",
                cik10,
                "SEC recent filings",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }
}

/// One recent filing from the submissions index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentFiling {
    /// The form type as EDGAR reports it (`10-Q`, `8-K`, `10-K/A`, `4`, …).
    pub form: String,
    /// The filing date, ISO.
    pub filing_date: String,
    /// The filer-declared item codes for an 8-K (`["4.01", "9.01"]` — the
    /// submissions feed's `items` column, split on commas), empty on other forms.
    /// Filer-declared structured metadata, mandatory in the 8-K submission types,
    /// so an 8-K row filed after 2004 reliably carries its items — what lets the
    /// hard-forensic producer classify an Item 4.01 / 4.02 filing without
    /// fetching the document (`docs/data-sources.md §SEC EDGAR`).
    /// `None` = the column was absent or this row's entry unreadable — the row is
    /// **unclassifiable**, which the forensic sweep must surface as `unknown`,
    /// never fold into a clean result (the fabricated-clear failure mode);
    /// `Some(vec![])` = the column served an honestly empty entry (a non-8-K).
    pub items: Option<Vec<String>>,
    /// The filing's accession number — the event record's source lineage.
    pub accession: String,
}

/// Shape the submissions body's `filings.recent` parallel arrays (`form[i]` ↔
/// `filingDate[i]`) into rows, newest first as EDGAR serves them. A body without the `filings.recent`
/// arrays is malformed or drifted (the submissions schema always carries them,
/// empty arrays included, for every filer) — `Err`, never an empty success, so
/// the sweep types the filing family `unknown` instead of reading
/// "no new filings" off a body it couldn't interpret. The form + date legs are
/// **strict**: unpaired arrays, a non-string leg, or an undatable
/// `filingDate` all `Err` rather than dropping the row — a silently dropped
/// 8-K would fold into a clean forensic sweep, and a garbage date compares
/// lexically against the classifier's lookback bound (the fabricated-clear
/// rule, `crate::portfolio::ForensicFilingState`). The `accessionNumber`
/// column stays lenient per row (absent → empty lineage); the `items` column
/// is honest per row — an absent column or an unreadable entry yields `None`
/// (unclassifiable), never an empty list.
fn recent_filings_from_value(value: &Value) -> Result<Vec<RecentFiling>> {
    let recent = &value["filings"]["recent"];
    let (Some(forms), Some(dates)) = (recent["form"].as_array(), recent["filingDate"].as_array())
    else {
        anyhow::bail!(
            "SEC submissions body lacked the filings.recent arrays — malformed or drifted response"
        );
    };
    if forms.len() != dates.len() {
        anyhow::bail!(
            "SEC submissions form/filingDate arrays are unpaired ({} vs {}) — malformed or \
             drifted response",
            forms.len(),
            dates.len()
        );
    }
    let items = recent["items"].as_array();
    let accessions = recent["accessionNumber"].as_array();
    forms
        .iter()
        .zip(dates.iter())
        .enumerate()
        .map(|(i, (form, date))| {
            let form = form
                .as_str()
                .with_context(|| format!("SEC submissions row {i}: non-string form leg"))?;
            let filing_date = date
                .as_str()
                .with_context(|| format!("SEC submissions row {i}: non-string filingDate leg"))?;
            // Store the CANONICAL fixed-width render, never the source text:
            // chrono accepts unpadded fields, and a datable-but-noncanonical
            // "2026-9-30" sorts lexically after "2026-10-01" — exactly the
            // comparison the forensic lookback bound makes (the same hazard
            // `fmp::canonical_date` guards; Codex 2026-08-20 round 3).
            let filing_date = chrono::NaiveDate::parse_from_str(filing_date, "%Y-%m-%d")
                .with_context(|| {
                    format!("SEC submissions row {i}: undatable filingDate {filing_date:?}")
                })?
                .format("%Y-%m-%d")
                .to_string();
            Ok(RecentFiling {
                form: form.to_string(),
                filing_date,
                items: items
                    .and_then(|a| a.get(i))
                    .and_then(Value::as_str)
                    .map(|s| {
                        s.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect()
                    }),
                accession: accessions
                    .and_then(|a| a.get(i))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

// ---- Hard-forensic filing kinds (the item-classified producer) -----------------

/// The typed hard-forensic event kinds — the shared producer contract
/// (`docs/trade-opportunities-workflow.md §Step 5c`; Portfolio's engine and
/// continuity seams consume the same records).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForensicEventKind {
    /// An Item 4.02 non-reliance 8-K.
    Restatement,
    /// An Item 4.01 auditor-change 8-K.
    AuditorChange,
    /// The research-fed kind — no structured enumeration exists, so it enters
    /// only as a validated `forensic_event` research claim (the 6d channel,
    /// merged by `pipeline::merge_research_forensic_event`). This filings
    /// classifier never emits it.
    Fraud,
}

impl ForensicEventKind {
    pub fn label(self) -> &'static str {
        match self {
            ForensicEventKind::Restatement => "restatement (Item 4.02 non-reliance)",
            ForensicEventKind::AuditorChange => "auditor change (Item 4.01)",
            ForensicEventKind::Fraud => "fraud (research-fed)",
        }
    }
}

/// One typed hard-forensic event — `{ event kind, issuer, event / filing date,
/// source lineage, confidence }`, the producer contract's record. The filing
/// kinds are engine-detected and model-free; a bare model assertion is never one
/// of these.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ForensicEvent {
    pub kind: ForensicEventKind,
    /// The issuer, as the consuming job identifies it (the holding's symbol).
    pub issuer: String,
    /// The filing date, ISO — the event date the hard rule's lookback reads.
    pub filing_date: String,
    /// Source lineage — the form type plus accession number of the classifying
    /// filing.
    pub source: String,
    /// How the event was established. Filing kinds are filer-declared item
    /// codes, structural rather than judged.
    pub confidence: String,
}

/// Classify the hard-forensic **filing kinds** from an already-fetched
/// submissions sweep: an 8-K (or 8-K/A) whose filer-declared items carry `4.02`
/// (non-reliance restatement) or `4.01` (auditor change), filed on or after
/// `since` (ISO date, inclusive — the consumer's lookback bound). Model-free and
/// pure. `Err` when any in-lookback 8-K row is **unclassifiable** (its `items`
/// read `None` — the column absent or the entry unreadable): the caller must
/// type the sweep `unknown`, because "couldn't read the items" folded into a
/// clean result is exactly the fabricated clear the contract forbids. The fraud
/// kind never comes from here (research-fed only).
pub fn forensic_events_from_filings(
    issuer: &str,
    filings: &[RecentFiling],
    since: &str,
) -> std::result::Result<Vec<ForensicEvent>, String> {
    let in_scope = filings.iter().filter(|f| {
        (f.form == "8-K" || f.form == "8-K/A") && f.filing_date.as_str() >= since
    });
    let mut events = Vec::new();
    for f in in_scope {
        let Some(items) = &f.items else {
            return Err(format!(
                "8-K filed {} carries no readable items column — cannot classify",
                f.filing_date
            ));
        };
        for item in items {
            let kind = match item.as_str() {
                "4.02" => ForensicEventKind::Restatement,
                "4.01" => ForensicEventKind::AuditorChange,
                _ => continue,
            };
            events.push(ForensicEvent {
                kind,
                issuer: issuer.to_string(),
                filing_date: f.filing_date.clone(),
                source: if f.accession.is_empty() {
                    format!("{} filing", f.form)
                } else {
                    format!("{} accession {}", f.form, f.accession)
                },
                confidence: "filing-declared item code".to_string(),
            });
        }
    }
    Ok(events)
}

/// Candidate GAAP concept names for revenue — the tag changed across taxonomy
/// versions, so try the newer name first and fall back.
const REVENUE_CONCEPTS: &[&str] = &[
    "RevenueFromContractWithCustomerExcludingAssessedTax",
    "Revenues",
    "SalesRevenueNet",
];

/// Shape an `/api/xbrl/companyfacts` body into [`CompanyFacts`]. Pure, so the
/// envelope contract is unit-testable without a live call. The prior-year revenue
/// deliberately comes from the **same concept ladder rung** that supplied the latest
/// print — a growth read across two different revenue tags would compare different
/// economics.
fn facts_from_value(value: &Value) -> CompanyFacts {
    let (revenue, revenue_prior) = REVENUE_CONCEPTS
        .iter()
        .find_map(|c| {
            let (latest, prior) = latest_two_annual_usd(value, c);
            latest.map(|l| (Some(l), prior))
        })
        .unwrap_or((None, None));
    CompanyFacts {
        revenue,
        revenue_prior,
        gross_profit: latest_annual_usd(value, "GrossProfit"),
        net_income: latest_annual_usd(value, "NetIncomeLoss"),
        total_assets: latest_annual_usd(value, "Assets"),
        stockholders_equity: latest_annual_usd(value, "StockholdersEquity"),
    }
}

/// The latest annual (form `10-K`, full-year) USD value for one GAAP concept, picked
/// by the most recent `end` date. `None` when the concept is absent or has no annual
/// USD datapoint. Reading only the 10-K full-year rows avoids mixing a quarterly
/// figure into an annual metric.
fn latest_annual_usd(value: &Value, concept: &str) -> Option<i64> {
    let (latest, _) = latest_two_annual_usd(value, concept);
    latest
}

/// Instant (balance) concepts — a point-in-time fact carries no `start`, so the
/// annual-duration span check does not apply. Everything else is treated as a
/// duration (flow) concept and FAILS CLOSED without a parseable ~annual span:
/// the safe default for any concept added later, since a mistakenly-instant
/// read only skips a check while a mistakenly-duration read fabricates an
/// annual value from a stub period.
const INSTANT_CONCEPTS: &[&str] = &["Assets", "StockholdersEquity"];

/// The latest **two** annual full-year USD values for one GAAP concept, by distinct
/// `end` date descending — the second is the prior fiscal year's print (a 10-K
/// carries its comparative-year rows under the same concept, which is what makes the
/// prior read possible without a second request). Duplicate rows for one period end
/// (an original filing plus a later 10-K's comparative) collapse to the
/// **latest-filed** row — a later filing that restates the period (recast for a
/// disposal, an accounting change) supersedes the original print; rows without a
/// `filed` date fall back to array order, where EDGAR lists later filings later.
fn latest_two_annual_usd(value: &Value, concept: &str) -> (Option<i64>, Option<i64>) {
    let Some(units) = value
        .pointer(&format!("/facts/us-gaap/{concept}/units/USD"))
        .and_then(Value::as_array)
    else {
        return (None, None);
    };
    // (end, filed, array index, val) — sorted so the row to keep per period end
    // comes first: end desc, then filed desc (absent filed sorts behind any
    // present one), then array position desc as the filing-order proxy.
    let mut dated: Vec<(String, Option<String>, usize, i64)> = units
        .iter()
        .enumerate()
        // Prefix match: a 10-K/A restating the year is the most direct
        // supersession vehicle — an exact "10-K" test would keep serving the
        // withdrawn original print until the next annual report's comparative.
        .filter(|(_, row)| {
            row.get("form")
                .and_then(Value::as_str)
                .is_some_and(|f| f.starts_with("10-K"))
        })
        // Prefer full-year datapoints; many 10-K rows carry `"fp":"FY"`.
        .filter(|(_, row)| {
            row.get("fp")
                .and_then(Value::as_str)
                .map(|fp| fp == "FY")
                .unwrap_or(true)
        })
        // Duration facts must span roughly a year: company-facts arrays mix
        // sub-annual durations under the same concept and form, and a Q4/stub
        // row sharing the FY `end` date would otherwise win on a tie-break and
        // masquerade as the annual value. CONCEPT-AWARE and fail-closed: a
        // duration concept's row with an absent or unparseable `start` is
        // excluded (a pass-through would readmit exactly the stub rows the
        // filter exists to stop), while instant concepts skip the check —
        // point-in-time facts legitimately carry no `start`.
        .filter(|(_, row)| {
            if INSTANT_CONCEPTS.contains(&concept) {
                return true;
            }
            row.get("start")
                .and_then(Value::as_str)
                .zip(row.get("end").and_then(Value::as_str))
                .and_then(|(s, e)| {
                    let s = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
                    let e = chrono::NaiveDate::parse_from_str(e, "%Y-%m-%d").ok()?;
                    Some((e - s).num_days())
                })
                .is_some_and(|d| (350..=380).contains(&d))
        })
        .filter_map(|(idx, row)| {
            let end = row.get("end").and_then(Value::as_str)?;
            let filed = row
                .get("filed")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            let val = row.get("val").and_then(Value::as_i64)?;
            Some((end.to_string(), filed, idx, val))
        })
        .collect();
    dated.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| b.2.cmp(&a.2))
    });
    dated.dedup_by(|a, b| a.0 == b.0);
    let mut iter = dated.into_iter();
    (
        iter.next().map(|(_, _, _, v)| v),
        iter.next().map(|(_, _, _, v)| v),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    fn facts_body() -> &'static str {
        // Two revenue datapoints (older + newer 10-K, the older year appearing twice:
        // the original print and a later-filed 10-K comparative that RESTATES it) and
        // one each for the rest. The parser must pick the latest annual by `end`
        // date, the prior year from the same concept's second-distinct end, and the
        // latest-FILED row where one period end appears twice.
        r#"{
          "facts": {
            "us-gaap": {
              "RevenueFromContractWithCustomerExcludingAssessedTax": {
                "units": { "USD": [
                  {"start":"2023-10-01","end":"2024-09-28","val":391035000000,"form":"10-K","fp":"FY","filed":"2024-11-01"},
                  {"start":"2022-10-02","end":"2023-09-30","val":383285000000,"form":"10-K","fp":"FY","filed":"2023-11-03"},
                  {"start":"2022-10-02","end":"2023-09-30","val":380000000000,"form":"10-K","fp":"FY","filed":"2024-11-01"}
                ]}
              },
              "Revenues": {
                "units": { "USD": [
                  {"start":"2021-09-26","end":"2022-09-24","val":394328000000,"form":"10-K","fp":"FY"}
                ]}
              },
              "NetIncomeLoss": {
                "units": { "USD": [
                  {"start":"2023-10-01","end":"2024-09-28","val":93736000000,"form":"10-K","fp":"FY"},
                  {"start":"2024-03-31","end":"2024-06-29","val":21448000000,"form":"10-Q","fp":"Q3"}
                ]}
              },
              "StockholdersEquity": {
                "units": { "USD": [ {"end":"2024-09-28","val":56950000000,"form":"10-K","fp":"FY"} ] }
              }
            }
          }
        }"#
    }

    #[test]
    fn parses_latest_annual_facts_and_ignores_quarterly_rows() {
        let value: Value = serde_json::from_str(facts_body()).unwrap();
        let facts = facts_from_value(&value);
        // Latest annual revenue (the 2024 10-K), not the prior year.
        assert_eq!(facts.revenue, Some(391_035_000_000));
        // The prior year comes from the SAME concept's second-distinct end; the
        // duplicated period end collapses to the latest-FILED row (the later 10-K's
        // restated comparative supersedes the original print), and the older
        // `Revenues` concept (a different tag, different economics) never mixes in.
        assert_eq!(facts.revenue_prior, Some(380_000_000_000));
        // The 10-Q net-income row is filtered out; the 10-K stands.
        assert_eq!(facts.net_income, Some(93_736_000_000));
        assert_eq!(facts.stockholders_equity, Some(56_950_000_000));
        // A concept that wasn't reported stays absent rather than fabricated.
        assert_eq!(facts.total_assets, None);
        assert!(!facts.is_empty());
    }

    #[test]
    fn amendments_supersede_and_sub_annual_durations_are_excluded() {
        // A 10-K/A restating the year is the most direct supersession vehicle —
        // the prefix match must let its later-filed row win the period-end
        // dedup — and a Q4-duration fact sharing the FY `end` date on a 10-K
        // row must not masquerade as the annual value (the tie-break would
        // otherwise fall to serialization order).
        let body = r#"{
          "facts": {
            "us-gaap": {
              "RevenueFromContractWithCustomerExcludingAssessedTax": {
                "units": { "USD": [
                  {"start":"2023-10-01","end":"2024-09-28","val":391035000000,"form":"10-K","fp":"FY","filed":"2024-11-01"},
                  {"start":"2023-10-01","end":"2024-09-28","val":359752200000,"form":"10-K/A","fp":"FY","filed":"2025-03-15"},
                  {"start":"2024-06-30","end":"2024-09-28","val":94930000000,"form":"10-K","fp":"FY","filed":"2024-11-01"},
                  {"end":"2024-09-28","val":1,"form":"10-K","fp":"FY","filed":"2025-06-01"},
                  {"start":"2022-10-02","end":"2023-09-30","val":383285000000,"form":"10-K","fp":"FY","filed":"2023-11-03"}
                ]}
              }
            }
          }
        }"#;
        let value: Value = serde_json::from_str(body).unwrap();
        let facts = facts_from_value(&value);
        // The amendment's restated print wins (later filed, same period end);
        // the Q4-duration row (90 days) is excluded outright, and so is the
        // start-less duration row — filed latest, it would WIN the dedup under
        // a fail-open span check (duration concepts fail closed; only instant
        // concepts legitimately omit `start`).
        assert_eq!(facts.revenue, Some(359_752_200_000));
        assert_eq!(facts.revenue_prior, Some(383_285_000_000));
    }

    #[test]
    fn fetch_round_trips_a_200_into_company_facts() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: facts_body(),
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let facts = sec.fetch_company_facts("0000320193").unwrap();
        assert_eq!(facts.revenue, Some(391_035_000_000));
        assert_eq!(
            server.request_paths(),
            vec!["/api/xbrl/companyfacts/CIK0000320193.json".to_string()]
        );
    }

    #[test]
    fn fetch_surfaces_a_non_2xx_as_an_error() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 404,
            headers: vec![],
            body: "not found",
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let err = sec.fetch_company_facts("0000000000").unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");
    }

    #[test]
    fn recent_filings_round_trip_the_submissions_parallel_arrays() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"cik":"320193","filings":{"recent":{
                "form":["4","10-Q","8-K"],
                "filingDate":["2026-08-01","2026-07-31","2026-07-30"],
                "accessionNumber":["a","b","c"]
            }}}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let filings = sec.fetch_recent_filings("0000320193").unwrap();
        assert_eq!(filings.len(), 3);
        assert_eq!(filings[0].form, "4");
        assert_eq!(filings[1].form, "10-Q");
        assert_eq!(filings[1].filing_date, "2026-07-31");
        assert_eq!(
            server.request_paths(),
            vec!["/submissions/CIK0000320193.json".to_string()]
        );
    }

    #[test]
    fn recent_filings_parse_items_honestly_per_row() {
        // The items column is the 8-K classification surface (comma-separated,
        // filer-declared): a served entry parses (an empty entry is an honest
        // `Some(vec![])`), while an absent column reads `None` — unclassifiable,
        // for the forensic sweep to surface as unknown, never fold into clean.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"cik":"320193","filings":{"recent":{
                "form":["8-K","10-Q"],
                "filingDate":["2026-08-01","2026-07-31"],
                "items":["4.02,9.01",""],
                "accessionNumber":["0000320193-26-000042","0000320193-26-000041"]
            }}}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let filings = sec.fetch_recent_filings("0000320193").unwrap();
        assert_eq!(
            filings[0].items,
            Some(vec!["4.02".to_string(), "9.01".to_string()])
        );
        assert_eq!(filings[0].accession, "0000320193-26-000042");
        assert_eq!(filings[1].items, Some(vec![]));

        // No items / accession arrays at all: rows still parse (the form + date
        // hard floor holds), but every row's items read `None`.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"cik":"320193","filings":{"recent":{
                "form":["8-K"],
                "filingDate":["2026-08-01"]
            }}}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let filings = sec.fetch_recent_filings("0000320193").unwrap();
        assert_eq!(filings.len(), 1);
        assert_eq!(filings[0].items, None);
        assert!(filings[0].accession.is_empty());
    }

    #[test]
    fn malformed_form_or_date_legs_error_never_drop_the_row() {
        // A null date leg on an in-scope 8-K must not vanish into a clean
        // sweep, and a garbage date must not survive to compare lexically
        // against the classifier's lookback bound — the whole fetch errors
        // onto the callers' unknown postures (Codex 2026-08-20 round 2,
        // finding 1).
        let bodies = [
            // Null date leg.
            r#"{"filings":{"recent":{"form":["8-K"],"filingDate":[null]}}}"#,
            // Non-date date leg (lexically above any ISO lookback bound).
            r#"{"filings":{"recent":{"form":["8-K"],"filingDate":["not-a-date"]}}}"#,
            // Null form leg.
            r#"{"filings":{"recent":{"form":[null],"filingDate":["2026-08-01"]}}}"#,
            // Unpaired arrays.
            r#"{"filings":{"recent":{"form":["8-K","10-Q"],"filingDate":["2026-08-01"]}}}"#,
        ];
        for body in bodies {
            let server = MockHttp::serve(vec![Canned::Reply {
                status: 200,
                headers: vec![],
                body: Box::leak(body.to_string().into_boxed_str()),
            }]);
            let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
            assert!(sec.fetch_recent_filings("0000320193").is_err(), "{body}");
        }

        // A datable-but-noncanonical date is stored in its canonical render —
        // "2026-9-30" would otherwise sort lexically after "2026-10-01",
        // exactly the classifier's lookback comparison.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"filings":{"recent":{"form":["8-K"],"filingDate":["2026-9-30"]}}}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let filings = sec.fetch_recent_filings("0000320193").unwrap();
        assert_eq!(filings[0].filing_date, "2026-09-30");
    }

    #[test]
    fn forensic_classifier_types_401_402_inside_the_lookback_only() {
        let filing = |form: &str, date: &str, items: &[&str]| RecentFiling {
            form: form.into(),
            filing_date: date.into(),
            items: Some(items.iter().map(|s| s.to_string()).collect()),
            accession: "acc-1".into(),
        };
        let filings = vec![
            // Item 4.02 → restatement; the companion 9.01 produces nothing.
            filing("8-K", "2026-08-01", &["4.02", "9.01"]),
            // An amended 8-K classifies too.
            filing("8-K/A", "2026-07-15", &["4.01"]),
            // A non-forensic 8-K item → no event.
            filing("8-K", "2026-07-10", &["2.02"]),
            // Outside the lookback → filtered.
            filing("8-K", "2024-01-05", &["4.02"]),
            // A 10-Q never classifies, whatever its items column carries.
            filing("10-Q", "2026-07-31", &["4.02"]),
        ];
        let events = forensic_events_from_filings("ACME", &filings, "2025-08-20").unwrap();
        assert_eq!(events.len(), 2, "{events:?}");
        assert_eq!(events[0].kind, ForensicEventKind::Restatement);
        assert_eq!(events[0].issuer, "ACME");
        assert_eq!(events[0].filing_date, "2026-08-01");
        assert!(events[0].source.contains("acc-1"), "{}", events[0].source);
        assert_eq!(events[1].kind, ForensicEventKind::AuditorChange);
        // Tightening the lookback drops the older auditor change, keeping only
        // the newer restatement — the bound is inclusive on `since`.
        assert_eq!(
            forensic_events_from_filings("ACME", &filings, "2026-07-20")
                .unwrap()
                .len(),
            1
        );

        // An in-lookback 8-K whose items column is unreadable makes the whole
        // sweep unclassifiable — `Err`, never a clean or partial result (the
        // fabricated-clear rule). Outside the lookback it is ignorable.
        let mut with_unreadable = filings.clone();
        with_unreadable.push(RecentFiling {
            form: "8-K".into(),
            filing_date: "2026-08-10".into(),
            items: None,
            accession: String::new(),
        });
        assert!(forensic_events_from_filings("ACME", &with_unreadable, "2025-08-20").is_err());
        assert!(forensic_events_from_filings("ACME", &with_unreadable, "2026-08-11").is_ok());
    }

    #[test]
    fn recent_filings_error_on_non_2xx_never_reading_as_no_new_filings() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 404,
            headers: vec![],
            body: "not found",
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        assert!(sec.fetch_recent_filings("0000000000").is_err());
    }

    #[test]
    fn recent_filings_error_on_a_200_missing_the_recent_arrays() {
        // Valid JSON without `filings.recent` is schema drift or a malformed
        // response — `Err`, never an empty "no new filings" success.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"cik":"320193","name":"Apple Inc."}"#,
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let err = sec.fetch_recent_filings("0000320193").unwrap_err();
        assert!(err.to_string().contains("filings.recent"), "{err}");
    }

    fn tickers_body() -> &'static str {
        r#"{
          "0": {"cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc."},
          "1": {"cik_str": 789019, "ticker": "MSFT", "title": "MICROSOFT CORP"},
          "2": {"cik_str": 34088, "ticker": "XOM", "title": "EXXON MOBIL CORP"},
          "3": {"ticker": "BROKEN"}
        }"#
    }

    #[test]
    fn resolver_parses_the_full_map_and_zero_pads_ciks() {
        let resolver = CikResolver::from_json(tickers_body()).unwrap();
        assert_eq!(resolver.len(), 3, "the malformed row is skipped, not fabricated");
        assert_eq!(resolver.resolve("aapl"), Some("0000320193"));
        assert_eq!(resolver.resolve("XOM"), Some("0000034088"), "short CIKs zero-pad to 10");
        assert_eq!(resolver.resolve("ZZZZ"), None);
    }

    #[test]
    fn ticker_map_fetch_round_trips_and_hits_the_files_path() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: tickers_body(),
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let body = sec.fetch_company_tickers().unwrap();
        assert!(CikResolver::from_json(&body).unwrap().resolve("MSFT").is_some());
        assert_eq!(
            server.request_paths(),
            vec!["/files/company_tickers.json".to_string()]
        );
    }

    #[test]
    fn load_cik_resolver_fetches_then_reuses_the_fresh_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sec_company_tickers.json");
        // First load: no cache → fetch → cache written.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: tickers_body(),
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let resolver = load_cik_resolver(&cache, &sec);
        assert_eq!(resolver.resolve("AAPL"), Some("0000320193"));
        assert!(cache.exists(), "the fetched map is cached beside the db");
        // Second load: the fresh cache serves without any request — the mock has no
        // second canned reply, so a fetch attempt would fail and fall to empty.
        let sec_offline = SecEdgarSource::new().unwrap().with_base_url("http://127.0.0.1:1");
        let resolver = load_cik_resolver(&cache, &sec_offline);
        assert_eq!(resolver.resolve("MSFT"), Some("0000789019"));
    }

    #[test]
    fn load_cik_resolver_falls_back_to_a_stale_cache_then_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sec_company_tickers.json");
        std::fs::write(&cache, tickers_body()).unwrap();
        // Age the cache past the freshness window so a refresh is attempted; the
        // unreachable fetch then falls back to the stale cache rather than empty.
        age_past_freshness(&cache);
        let sec_offline = SecEdgarSource::new().unwrap().with_base_url("http://127.0.0.1:1");
        let resolver = load_cik_resolver(&cache, &sec_offline);
        assert_eq!(resolver.resolve("AAPL"), Some("0000320193"), "stale beats empty");
        // No cache at all → the empty fail-soft floor.
        let resolver = load_cik_resolver(&dir.path().join("missing.json"), &sec_offline);
        assert!(resolver.is_empty());
    }

    /// Age a cache file past the freshness window so a refresh is attempted.
    fn age_past_freshness(cache: &std::path::Path) {
        let stale = std::time::SystemTime::now() - (CIK_CACHE_MAX_AGE + Duration::from_secs(60));
        let file = std::fs::File::options().append(true).open(cache).unwrap();
        file.set_modified(stale).unwrap();
    }

    /// Documents the bail: the ticker-map refresh honors the shared cancel flag
    /// like every SEC request, so under a set flag it makes **no request** and
    /// falls to the stale map. This is exactly why the live jobs must not load
    /// the resolver before the slot clears the flag (`reset_cancel`) — an eager
    /// load after a cancelled run would ship this stale (or empty) map into the
    /// whole run without a single request row.
    #[test]
    fn load_cik_resolver_under_a_set_cancel_flag_and_stale_cache_returns_the_stale_map() {
        use std::sync::atomic::AtomicBool;
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sec_company_tickers.json");
        std::fs::write(&cache, tickers_body()).unwrap();
        age_past_freshness(&cache);
        // A mock that WOULD serve a fresh map — it must see no connection.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: tickers_body(),
        }]);
        let cancelled = RunContext::new(
            "cancelled-earlier",
            Arc::new(crate::progress::NoopReporter),
            Arc::new(AtomicBool::new(true)),
        );
        let sec = SecEdgarSource::new()
            .unwrap()
            .with_base_url(&server.base_url)
            .with_context(cancelled);
        let resolver = load_cik_resolver(&cache, &sec);
        assert_eq!(
            resolver.resolve("AAPL"),
            Some("0000320193"),
            "the stale cache is served, not empty"
        );
        assert_eq!(server.attempts(), 0, "a set cancel flag skips the refresh fetch");
        // With no cache the same bail lands on the empty floor.
        let resolver = load_cik_resolver(&dir.path().join("missing.json"), &sec);
        assert!(resolver.is_empty());
        assert_eq!(server.attempts(), 0);
    }

    /// The lazy carrier: constructing it performs no I/O; the first `get` loads
    /// (one request), the second serves the memoized map (no request).
    #[test]
    fn lazy_cik_resolver_fetches_on_first_use_only() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sec_company_tickers.json");
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: tickers_body(),
        }]);
        let sec = SecEdgarSource::new().unwrap().with_base_url(&server.base_url);
        let lazy = LazyCikResolver::new(&cache);
        assert!(!lazy.is_loaded());
        assert_eq!(server.attempts(), 0, "construction fetches nothing");
        assert_eq!(lazy.resolve(&sec, "MSFT"), Some("0000789019"));
        assert!(lazy.is_loaded());
        assert_eq!(server.attempts(), 1);
        // Memoized: no second connection (the mock has no second reply anyway).
        assert_eq!(lazy.resolve(&sec, "AAPL"), Some("0000320193"));
        assert_eq!(server.attempts(), 1);
        // A preloaded resolver never touches the source.
        let pre = LazyCikResolver::preloaded(CikResolver::from_json(tickers_body()).unwrap());
        assert!(pre.is_loaded());
        assert_eq!(pre.resolve(&sec, "XOM"), Some("0000034088"));
        assert_eq!(server.attempts(), 1);
    }

    #[test]
    fn cik_cache_lives_beside_the_database() {
        assert_eq!(
            cik_cache_path_beside(std::path::Path::new("/data/app/market_signal.db")),
            std::path::PathBuf::from("/data/app/sec_company_tickers.json")
        );
        assert_eq!(
            cik_cache_path_beside(std::path::Path::new("market_signal.db")),
            std::path::PathBuf::from("sec_company_tickers.json")
        );
    }
}
