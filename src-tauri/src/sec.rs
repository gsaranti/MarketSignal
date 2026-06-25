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
    pub gross_profit: Option<i64>,
    pub net_income: Option<i64>,
    pub total_assets: Option<i64>,
    pub stockholders_equity: Option<i64>,
}

impl CompanyFacts {
    /// Whether any fact resolved — the dossier uses this to decide if SEC contributed.
    pub fn is_empty(&self) -> bool {
        self.revenue.is_none()
            && self.gross_profit.is_none()
            && self.net_income.is_none()
            && self.total_assets.is_none()
            && self.stockholders_equity.is_none()
    }
}

/// Static ticker → CIK resolution for the names this slice runs against. The dynamic
/// `company_tickers.json` resolution (the full ~10k-symbol map) is a later slice; for
/// the single-equity fixture a small table keeps the path offline and fast. Returns
/// the 10-digit zero-padded CIK EDGAR expects.
pub fn cik_for_ticker(ticker: &str) -> Option<&'static str> {
    match ticker.to_ascii_uppercase().as_str() {
        "AAPL" => Some("0000320193"),
        "MSFT" => Some("0000789019"),
        "NVDA" => Some("0001045810"),
        "GOOGL" | "GOOG" => Some("0001652044"),
        "AMZN" => Some("0001018724"),
        _ => None,
    }
}

/// The keyless SEC EDGAR company-facts adapter. Mirrors the gated adapters' shape
/// (`http` + `base_url` + `progress`), minus the API key.
pub struct SecEdgarSource {
    http: reqwest::blocking::Client,
    base_url: String,
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
            progress: RunContext::noop(),
        })
    }

    /// Point the adapter at a mock base URL for the offline round-trip test. Trailing
    /// slash trimmed so the joined path's leading slash doesn't double up.
    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
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
/// envelope contract is unit-testable without a live call.
fn facts_from_value(value: &Value) -> CompanyFacts {
    let revenue = REVENUE_CONCEPTS
        .iter()
        .find_map(|c| latest_annual_usd(value, c));
    CompanyFacts {
        revenue,
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
    let units = value
        .pointer(&format!("/facts/us-gaap/{concept}/units/USD"))
        .and_then(Value::as_array)?;
    units
        .iter()
        .filter(|row| row.get("form").and_then(Value::as_str) == Some("10-K"))
        // Prefer full-year datapoints; many 10-K rows carry `"fp":"FY"`.
        .filter(|row| {
            row.get("fp")
                .and_then(Value::as_str)
                .map(|fp| fp == "FY")
                .unwrap_or(true)
        })
        .filter_map(|row| {
            let end = row.get("end").and_then(Value::as_str)?;
            let val = row.get("val").and_then(Value::as_i64)?;
            Some((end.to_string(), val))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, val)| val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    fn facts_body() -> &'static str {
        // Two revenue datapoints (older + newer 10-K) and one each for the rest. The
        // parser must pick the latest annual by `end` date.
        r#"{
          "facts": {
            "us-gaap": {
              "RevenueFromContractWithCustomerExcludingAssessedTax": {
                "units": { "USD": [
                  {"end":"2024-09-28","val":391035000000,"form":"10-K","fp":"FY"},
                  {"end":"2023-09-30","val":383285000000,"form":"10-K","fp":"FY"}
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

    #[test]
    fn cik_resolution_covers_the_fixture_symbol() {
        assert_eq!(cik_for_ticker("aapl"), Some("0000320193"));
        assert!(cik_for_ticker("ZZZZ").is_none());
    }
}
