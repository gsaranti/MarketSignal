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

/// Load the ticker → CIK resolver: the on-disk cache when fresh
/// ([`CIK_CACHE_MAX_AGE`]), else a fetch that rewrites the cache. Fail-soft at every
/// step — a failed fetch falls back to a stale cache when one exists, and to the
/// empty resolver when none does, so an SEC outage degrades filings coverage to
/// typed gaps rather than blocking the run.
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
    /// hosts at the mock, since a test exercises one endpoint at a time.
    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
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
        .filter(|(_, row)| row.get("form").and_then(Value::as_str) == Some("10-K"))
        // Prefer full-year datapoints; many 10-K rows carry `"fp":"FY"`.
        .filter(|(_, row)| {
            row.get("fp")
                .and_then(Value::as_str)
                .map(|fp| fp == "FY")
                .unwrap_or(true)
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
                  {"end":"2024-09-28","val":391035000000,"form":"10-K","fp":"FY","filed":"2024-11-01"},
                  {"end":"2023-09-30","val":383285000000,"form":"10-K","fp":"FY","filed":"2023-11-03"},
                  {"end":"2023-09-30","val":380000000000,"form":"10-K","fp":"FY","filed":"2024-11-01"}
                ]}
              },
              "Revenues": {
                "units": { "USD": [
                  {"end":"2022-09-24","val":394328000000,"form":"10-K","fp":"FY"}
                ]}
              },
              "NetIncomeLoss": {
                "units": { "USD": [
                  {"end":"2024-09-28","val":93736000000,"form":"10-K","fp":"FY"},
                  {"end":"2024-06-29","val":21448000000,"form":"10-Q","fp":"Q3"}
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
        let stale = std::time::SystemTime::now() - (CIK_CACHE_MAX_AGE + Duration::from_secs(60));
        let file = std::fs::File::options().append(true).open(&cache).unwrap();
        file.set_modified(stale).unwrap();
        let sec_offline = SecEdgarSource::new().unwrap().with_base_url("http://127.0.0.1:1");
        let resolver = load_cik_resolver(&cache, &sec_offline);
        assert_eq!(resolver.resolve("AAPL"), Some("0000320193"), "stale beats empty");
        // No cache at all → the empty fail-soft floor.
        let resolver = load_cik_resolver(&dir.path().join("missing.json"), &sec_offline);
        assert!(resolver.is_empty());
    }
}
