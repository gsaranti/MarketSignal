//! Real Tavily adapter for Step-7 news ingestion (`NewsSource`).
//!
//! Tavily is the primary research / news-ingestion source (`docs/data-sources.md`
//! §Tavily) — it contributes AI-oriented market and research headlines. This
//! adapter issues one `/search` request per `news::NEWS_TOPICS` topic and maps
//! the results into provider-agnostic `RawHeadline`s.
//!
//! Like `fmp`, the HTTP call is synchronous (`reqwest::blocking`) so the
//! `NewsSource` trait stays sync; the blocking work is offloaded via
//! `spawn_blocking` at the Tauri command seam. The key rides as a Bearer token —
//! the convention `connection_test` already uses for Tavily.
//!
//! Degradation policy. Unlike `fmp`'s per-symbol skip, Tavily is a required,
//! gated provider credential (`config::validate` blocks a run without it) with no
//! "partial absence" case, so any non-2xx fails the gather loudly rather than
//! returning a thinned set: 401/403 a rejected key, 429/5xx an availability
//! problem, anything else unexpected.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::news::{host_of, NewsSource, RawHeadline, NEWS_TOPICS};

const TAVILY_SEARCH_URL: &str = "https://api.tavily.com/search";

/// Per-request timeout. The gather issues one request per topic sequentially;
/// none should park for the model adapter's 120s ceiling.
const TAVILY_TIMEOUT: Duration = Duration::from_secs(20);

/// Results requested per topic query. With ~7 topics this gathers on the order of
/// ~140 raw headlines from Tavily before dedup — well inside Step 7's ~500 funnel
/// budget once GDELT is added. 20 is Tavily's `basic` per-query ceiling.
const RESULTS_PER_QUERY: u32 = 20;

/// One Tavily search result, trimmed to the fields a headline needs. `title` and
/// `url` are what the filter keys on; `content` is the excerpt Tavily returns and
/// `published_date` rides only on the `news` topic. All default so a result
/// missing an optional field still parses.
#[derive(Debug, Deserialize)]
struct TavilyResultRaw {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    published_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TavilySearchResponse {
    #[serde(default)]
    results: Vec<TavilyResultRaw>,
}

/// Interpret a Tavily HTTP response by status, parsing the body only on a 2xx.
/// Any non-2xx is fatal to the gather (see the module header's degradation note).
fn interpret_tavily(status: u16, body: &str) -> Result<TavilySearchResponse> {
    match status {
        200..=299 => {
            serde_json::from_str(body).context("Tavily returned an unparseable 2xx body")
        }
        401 | 403 => bail!("Tavily rejected the key (HTTP {status})"),
        429 | 500..=599 => bail!(
            "Tavily is unavailable (HTTP {status}) — failing the news gather rather than \
             returning a partial set"
        ),
        _ => bail!("Tavily returned an unexpected response (HTTP {status})"),
    }
}

/// Shape a parsed Tavily response into raw headlines, dropping any result missing
/// a URL or title (nothing the filter could key on). The publisher `source` is
/// the URL's host; the snippet is Tavily's `content` excerpt when present.
fn headlines_from_response(resp: TavilySearchResponse) -> Vec<RawHeadline> {
    resp.results
        .into_iter()
        .filter(|r| !r.url.trim().is_empty() && !r.title.trim().is_empty())
        .map(|r| RawHeadline {
            source: host_of(&r.url),
            title: r.title,
            url: r.url,
            published: r.published_date,
            snippet: if r.content.trim().is_empty() {
                None
            } else {
                Some(r.content)
            },
        })
        .collect()
}

/// Live Tavily adapter behind the `NewsSource` trait.
pub struct TavilyNewsSource {
    api_key: String,
    http: reqwest::blocking::Client,
}

impl TavilyNewsSource {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(TAVILY_TIMEOUT)
            .build()
            .context("building the Tavily HTTP client")?;
        Ok(Self { api_key, http })
    }

    /// Resolve the adapter from the environment, for the live smoke and any caller
    /// that bypasses the gate. The execution gate (`config::validate`) runs ahead
    /// of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().tavily_key()?)
    }

    /// Search one topic, returning its headlines. A transport error (Tavily
    /// unreachable) or any non-2xx response propagates as a fatal gather error.
    fn search(&self, query: &str) -> Result<Vec<RawHeadline>> {
        let body = json!({
            "query": query,
            "topic": "news",
            "max_results": RESULTS_PER_QUERY,
            "search_depth": "basic",
        });
        let (status, text) = crate::http_retry::send_with_retry("Tavily", || {
            self.http
                .post(TAVILY_SEARCH_URL)
                .bearer_auth(&self.api_key)
                .json(&body)
        })?;
        Ok(headlines_from_response(interpret_tavily(status, &text)?))
    }
}

impl NewsSource for TavilyNewsSource {
    fn gather(&self) -> Result<Vec<RawHeadline>> {
        let mut out = Vec::new();
        for topic in NEWS_TOPICS {
            out.extend(self.search(topic)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_tavily_covers_the_status_matrix() {
        // A 2xx body parses into results.
        let body = r#"{"results":[{"title":"t","url":"https://x.com/a","content":"c"}]}"#;
        assert_eq!(interpret_tavily(200, body).unwrap().results.len(), 1);
        // A 2xx with no results array still parses (serde default -> empty).
        assert!(interpret_tavily(200, "{}").unwrap().results.is_empty());

        // Auth / availability / unexpected statuses are all fatal.
        for status in [401, 403, 429, 500, 503, 400, 404] {
            assert!(interpret_tavily(status, "").is_err(), "HTTP {status} should be fatal");
        }
        // A 2xx that isn't valid JSON is a contract violation -> fatal.
        assert!(interpret_tavily(200, "not json").is_err());
    }

    #[test]
    fn headlines_from_response_maps_and_drops_incomplete_results() {
        let body = r#"{
            "results": [
                {"title":"Fed holds","url":"https://www.reuters.com/markets/fed","content":"excerpt","published_date":"2026-06-05"},
                {"title":"no url","url":"","content":"x"},
                {"title":"","url":"https://x.com/empty-title","content":"x"},
                {"title":"no snippet","url":"https://bloomberg.com/n","content":"  "}
            ]
        }"#;
        let resp = interpret_tavily(200, body).unwrap();
        let headlines = headlines_from_response(resp);
        assert_eq!(headlines.len(), 2, "the two incomplete results are dropped");

        assert_eq!(headlines[0].title, "Fed holds");
        assert_eq!(headlines[0].source, "reuters.com", "source is the URL host");
        assert_eq!(headlines[0].published.as_deref(), Some("2026-06-05"));
        assert_eq!(headlines[0].snippet.as_deref(), Some("excerpt"));

        // A blank content excerpt becomes None rather than an empty string.
        assert_eq!(headlines[1].title, "no snippet");
        assert_eq!(headlines[1].snippet, None);
    }

    #[test]
    #[ignore = "hits the live Tavily API; set TAVILY_API_KEY"]
    fn live_search_smoke() {
        let tavily = TavilyNewsSource::from_env().expect("TAVILY_API_KEY set");
        let headlines = tavily.search("US economy inflation and the Federal Reserve").unwrap();
        assert!(!headlines.is_empty(), "expected live Tavily results");
        assert!(headlines.iter().all(|h| !h.url.is_empty()));
    }
}
