//! GDELT DOC 2.0 adapter for news ingestion (`NewsSource`;
//! `docs/data-sources.md` §GDELT). Keyless, so it sits outside the execution
//! gate. One combined `ArtList` query per gather, never one per topic: GDELT
//! rate-limits at roughly one request per 5 seconds with an escalating IP
//! lockout, so back-to-back per-category requests get all but the first 429'd.
//! Fail-soft: a failing query degrades to no headlines; `gather` never errors.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::cadence::ReportCadence;
use crate::news::{host_of, NewsSource, RawHeadline};
use crate::progress::RunContext;

const GDELT_DOC_URL: &str = "https://api.gdeltproject.org/api/v2/doc/doc";

const GDELT_TIMEOUT: Duration = Duration::from_secs(20);

/// GDELT's `ArtList` ceiling.
const GDELT_MAX_RECORDS: &str = "250";

/// Coverage window on the first report, when there is no prior report to
/// measure an interval against; later runs size the window to the elapsed
/// interval (see [`gdelt_timespan`]).
const GDELT_DEFAULT_TIMESPAN: &str = "1w";

/// Floor and cap (days) on the cadence-sized window. The floor keeps a
/// same-day regeneration from requesting a window so narrow it returns almost
/// nothing; the cap holds a long gap to a month. App-layer tunables.
const GDELT_WINDOW_FLOOR_DAYS: i64 = 1;
const GDELT_WINDOW_CAP_DAYS: i64 = 31;

/// Elapsed days round *up* so a partial day still covers its whole span.
fn gdelt_timespan(elapsed_days: Option<f64>) -> String {
    match elapsed_days {
        None => GDELT_DEFAULT_TIMESPAN.to_string(),
        Some(d) => {
            let days = (d.ceil() as i64).clamp(GDELT_WINDOW_FLOOR_DAYS, GDELT_WINDOW_CAP_DAYS);
            format!("{days}d")
        }
    }
}

/// GDELT refuses requests without a descriptive User-Agent even at low volume
/// (gdelt-doc-api#22); reqwest sends none by default.
const GDELT_USER_AGENT: &str =
    "MarketSignal/0.1 (market signal report; news ingestion via GDELT DOC 2.0)";

/// The single broad query covering the news categories as GDELT keywords.
/// Multi-word terms are quoted so they match as phrases, not ANDed tokens.
const GDELT_QUERY: &str = r#"("stock market" OR tariffs OR "oil prices" OR inflation OR semiconductors OR geopolitics OR "Federal Reserve")"#;

/// All fields default so an article missing one still parses; rows without
/// `url` or `title` are dropped in mapping.
#[derive(Debug, Deserialize)]
struct GdeltArticleRaw {
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    seendate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GdeltDocResponse {
    #[serde(default)]
    articles: Vec<GdeltArticleRaw>,
}

/// GDELT returns an empty or whitespace body when a query matches nothing —
/// no articles, not an error.
fn headlines_from_body(body: &str) -> Result<Vec<RawHeadline>> {
    if body.trim().is_empty() {
        return Ok(Vec::new());
    }
    let resp: GdeltDocResponse =
        serde_json::from_str(body).context("GDELT returned an unparseable body")?;
    Ok(resp
        .articles
        .into_iter()
        .filter(|a| !a.url.trim().is_empty() && !a.title.trim().is_empty())
        .map(|a| RawHeadline {
            // Normalized like Tavily's URL host so `source` reads consistently
            // across providers.
            source: host_of(&a.domain),
            title: a.title,
            url: a.url,
            published: a.seendate,
            snippet: None,
        })
        .collect())
}

pub struct GdeltNewsSource {
    http: reqwest::blocking::Client,
    /// Defaults to a no-op; the live command attaches the real one via
    /// [`GdeltNewsSource::with_context`].
    progress: Arc<RunContext>,
}

impl GdeltNewsSource {
    pub fn new() -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(GDELT_TIMEOUT)
            .user_agent(GDELT_USER_AGENT)
            .build()
            .context("building the GDELT HTTP client")?;
        Ok(Self {
            http,
            progress: RunContext::noop(),
        })
    }

    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Stays honest about a failure; `gather` applies the fail-soft policy.
    fn search(&self, query: &str, timespan: &str) -> Result<Vec<RawHeadline>> {
        let resp = self
            .http
            .get(GDELT_DOC_URL)
            .query(&[
                ("query", query),
                ("mode", "ArtList"),
                ("format", "json"),
                ("maxrecords", GDELT_MAX_RECORDS),
                ("timespan", timespan),
                ("sort", "DateDesc"),
            ])
            .send()
            .context("sending GDELT request")?;
        let status = resp.status().as_u16();
        let text = resp.text().context("reading GDELT response body")?;
        if !(200..300).contains(&status) {
            bail!("GDELT returned HTTP {status}");
        }
        headlines_from_body(&text)
    }
}

impl NewsSource for GdeltNewsSource {
    /// Fail-soft: a failing query logs and degrades to no headlines.
    fn gather(&self, cadence: ReportCadence) -> Result<Vec<RawHeadline>> {
        if self.progress.is_cancelled() {
            return Ok(Vec::new());
        }
        let timespan = gdelt_timespan(cadence.elapsed_days());
        self.progress
            .request_started("GDELT", "news", "gdelt-sweep", "Geopolitical news sweep");
        match self.search(GDELT_QUERY, &timespan) {
            Ok(headlines) => {
                self.progress.request_finished(
                    "GDELT",
                    "news",
                    "gdelt-sweep",
                    "Geopolitical news sweep",
                    "ok",
                    None,
                );
                Ok(headlines)
            }
            Err(e) => {
                eprintln!("GDELT news gather degraded to empty: {e}");
                self.progress.request_finished(
                    "GDELT",
                    "news",
                    "gdelt-sweep",
                    "Geopolitical news sweep",
                    "failed",
                    Some(e.to_string()),
                );
                Ok(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headlines_from_body_maps_and_drops_incomplete_articles() {
        let body = r#"{
            "articles": [
                {"url":"https://reuters.com/markets/a","title":"Markets rally","domain":"WWW.Reuters.com","seendate":"20260605T120000Z"},
                {"url":"","title":"no url","domain":"x.com"},
                {"url":"https://x.com/empty","title":"","domain":"x.com"}
            ]
        }"#;
        let headlines = headlines_from_body(body).unwrap();
        assert_eq!(
            headlines.len(),
            1,
            "the two incomplete articles are dropped"
        );
        assert_eq!(headlines[0].title, "Markets rally");
        assert_eq!(
            headlines[0].source, "reuters.com",
            "source is the normalized domain"
        );
        assert_eq!(headlines[0].published.as_deref(), Some("20260605T120000Z"));
        assert_eq!(
            headlines[0].snippet, None,
            "GDELT ArtList carries no excerpt"
        );
    }

    #[test]
    fn empty_body_is_no_articles_not_an_error() {
        assert_eq!(headlines_from_body("").unwrap().len(), 0);
        assert_eq!(headlines_from_body("   ").unwrap().len(), 0);
        assert_eq!(headlines_from_body("{}").unwrap().len(), 0);
    }

    #[test]
    fn non_empty_garbage_body_is_an_error() {
        assert!(headlines_from_body("<html>rate limited</html>").is_err());
    }

    #[test]
    fn timespan_is_sized_to_the_elapsed_interval_and_clamped() {
        assert_eq!(gdelt_timespan(None), "1w");
        assert_eq!(gdelt_timespan(Some(0.3)), "1d");
        assert_eq!(gdelt_timespan(Some(7.0)), "7d");
        assert_eq!(gdelt_timespan(Some(6.2)), "7d");
        assert_eq!(gdelt_timespan(Some(120.0)), "31d");
        // Clock skew (a negative interval) floors to the minimum window.
        assert_eq!(gdelt_timespan(Some(-3.0)), "1d");
    }

    #[test]
    #[ignore = "hits the live GDELT API (keyless)"]
    fn live_gather_smoke() {
        let gdelt = GdeltNewsSource::new().unwrap();
        // gather never errors, so a vacuous `all()` on an empty result would pass
        // even when GDELT is rate-limited to nothing; require a non-empty result.
        let headlines = gdelt.gather(ReportCadence::from_elapsed(None)).unwrap();
        eprintln!("GDELT live gather returned {} headlines", headlines.len());
        assert!(
            !headlines.is_empty(),
            "GDELT returned no headlines — rate-limited, or the query was rejected"
        );
        assert!(headlines
            .iter()
            .all(|h| !h.url.is_empty() && !h.title.is_empty()));
    }
}
