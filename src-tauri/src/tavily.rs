//! Tavily adapter for news ingestion (`NewsSource`; `docs/data-sources.md`
//! §Tavily): one `/search` request per `news::NEWS_TOPICS` topic. Each call
//! fails loud on a non-2xx, but the `gather` sweep is per-topic fail-soft so
//! one bad topic never discards the rest, nor GDELT's headlines via
//! `CompositeNewsSource`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::cadence::ReportCadence;
use crate::news::{host_of, NewsSource, RawHeadline, NEWS_TOPICS};
use crate::progress::RunContext;

const TAVILY_BASE: &str = "https://api.tavily.com";
const TAVILY_SEARCH_PATH: &str = "/search";

/// Topics run sequentially; none should park for the model adapter's 120s
/// ceiling.
const TAVILY_TIMEOUT: Duration = Duration::from_secs(20);

/// Tavily's `basic` per-query ceiling.
const RESULTS_PER_QUERY: u32 = 20;

/// Floor and cap (days) on the cadence-sized recency window. The floor keeps a
/// same-day regeneration from asking for a sub-day window; the cap holds a
/// long gap to a month. App-layer tunables. The first report (no elapsed
/// interval) omits the window and takes Tavily's own default.
const TAVILY_WINDOW_FLOOR_DAYS: u32 = 1;
const TAVILY_WINDOW_CAP_DAYS: u32 = 30;

/// Rounds *up* so a partial day still covers its whole span.
fn tavily_window_days(elapsed_days: Option<f64>) -> Option<u32> {
    elapsed_days.map(|d| {
        let days = d.ceil().max(0.0) as u32;
        days.clamp(TAVILY_WINDOW_FLOOR_DAYS, TAVILY_WINDOW_CAP_DAYS)
    })
}

/// The Tavily `start_date` (`YYYY-MM-DD`): `reference` minus the cadence-sized
/// window, `None` on the first report. Tavily's recency parameter is
/// `start_date`/`end_date`; its `days` field is not part of the Search API.
fn tavily_start_date(reference: chrono::NaiveDate, elapsed_days: Option<f64>) -> Option<String> {
    tavily_window_days(elapsed_days).map(|days| {
        (reference - chrono::Duration::days(i64::from(days)))
            .format("%Y-%m-%d")
            .to_string()
    })
}

/// A `None` `start_date` is omitted from the body so Tavily applies its own
/// default. The unit-test seam for the wire shape: the localhost mock does not
/// capture POST bodies.
fn search_body(query: &str, start_date: Option<&str>) -> serde_json::Value {
    let mut body = json!({
        "query": query,
        "topic": "news",
        "max_results": RESULTS_PER_QUERY,
        "search_depth": "basic",
    });
    if let Some(start_date) = start_date {
        body["start_date"] = json!(start_date);
    }
    body
}

/// All fields default so a result missing one still parses; `published_date`
/// rides only on the `news` topic.
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

/// Parses the body only on a 2xx; any non-2xx is an error.
fn interpret_tavily(status: u16, body: &str) -> Result<TavilySearchResponse> {
    match status {
        200..=299 => serde_json::from_str(body).context("Tavily returned an unparseable 2xx body"),
        401 | 403 => bail!("Tavily rejected the key (HTTP {status})"),
        429 | 500..=599 => bail!(
            "Tavily is unavailable (HTTP {status}) — failing the news gather rather than \
             returning a partial set"
        ),
        _ => bail!("Tavily returned an unexpected response (HTTP {status})"),
    }
}

/// Drops any result missing a URL or title (nothing the filter could key on).
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

pub struct TavilyNewsSource {
    api_key: String,
    http: reqwest::blocking::Client,
    /// Defaults to [`TAVILY_BASE`]; a test overrides it via
    /// [`TavilyNewsSource::with_base_url`].
    base_url: String,
    /// Defaults to a no-op; the live command attaches the real one via
    /// [`TavilyNewsSource::with_context`].
    progress: Arc<RunContext>,
}

impl TavilyNewsSource {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(TAVILY_TIMEOUT)
            .build()
            .context("building the Tavily HTTP client")?;
        Ok(Self {
            api_key,
            http,
            base_url: TAVILY_BASE.to_string(),
            progress: RunContext::noop(),
        })
    }

    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// For the live smoke and any caller that bypasses the gate; the execution
    /// gate (`config::validate`) runs ahead of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().tavily_key()?)
    }

    /// Shared by the topic sweep (`gather`) and the executor's plan queries
    /// (`SearchBackend`). A transport error or any non-2xx is an error.
    fn run_search(&self, query: &str, start_date: Option<&str>) -> Result<Vec<RawHeadline>> {
        let body = search_body(query, start_date);
        let url = format!("{}{TAVILY_SEARCH_PATH}", self.base_url);
        let (status, text) = crate::http_retry::send_with_retry("Tavily", || {
            self.http.post(&url).bearer_auth(&self.api_key).json(&body)
        })?;
        Ok(headlines_from_response(interpret_tavily(status, &text)?))
    }
}

/// Per-topic fail-soft: a topic whose search errors degrades to no headlines,
/// with a `failed` tracker row so the loss stays visible, and the sweep
/// continues. Extracted from `gather` so the soft-skip is unit-testable
/// without a live HTTP client.
fn sweep_topics(
    topics: &[&str],
    progress: &RunContext,
    mut search: impl FnMut(&str) -> Result<Vec<RawHeadline>>,
) -> Vec<RawHeadline> {
    let mut out = Vec::new();
    for &topic in topics {
        if progress.is_cancelled() {
            break;
        }
        // The tracker row lives here, not in `run_search`: the executor brackets
        // its own `SearchBackend::search` calls, so sharing `run_search` does
        // not double-count.
        progress.request_started("Tavily", "news", topic, topic);
        match search(topic) {
            Ok(headlines) => {
                progress.request_finished("Tavily", "news", topic, topic, "ok", None);
                out.extend(headlines);
            }
            Err(e) => {
                eprintln!("Tavily news topic {topic:?} degraded to empty: {e}");
                progress.request_finished(
                    "Tavily",
                    "news",
                    topic,
                    topic,
                    "failed",
                    Some(e.to_string()),
                );
            }
        }
    }
    out
}

impl NewsSource for TavilyNewsSource {
    fn gather(&self, cadence: ReportCadence) -> Result<Vec<RawHeadline>> {
        // Deliberately the UTC date, not the ET session: this only anchors a
        // fetch range's bound over a window of days and keys no session
        // (`docs/data-sources.md`, the session-dating rule).
        let today = chrono::Utc::now().date_naive();
        let start_date = tavily_start_date(today, cadence.elapsed_days());
        Ok(sweep_topics(NEWS_TOPICS, &self.progress, |query| {
            self.run_search(query, start_date.as_deref())
        }))
    }
}

impl crate::research_executor::SearchBackend for TavilyNewsSource {
    /// No recency bound: a plan query targets a topic, not a time slice.
    fn search(&self, query: &str) -> Result<Vec<RawHeadline>> {
        self.run_search(query, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    #[test]
    fn interpret_tavily_covers_the_status_matrix() {
        let body = r#"{"results":[{"title":"t","url":"https://x.com/a","content":"c"}]}"#;
        assert_eq!(interpret_tavily(200, body).unwrap().results.len(), 1);
        assert!(interpret_tavily(200, "{}").unwrap().results.is_empty());

        for status in [401, 403, 429, 500, 503, 400, 404] {
            assert!(
                interpret_tavily(status, "").is_err(),
                "HTTP {status} should be fatal"
            );
        }
        assert!(interpret_tavily(200, "not json").is_err());
    }

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----

    fn test_source(base_url: &str) -> TavilyNewsSource {
        TavilyNewsSource::new("test-key".to_string())
            .expect("build adapter")
            .with_base_url(base_url)
    }

    #[test]
    fn window_days_are_sized_to_the_elapsed_interval_and_clamped() {
        assert_eq!(tavily_window_days(None), None);
        assert_eq!(tavily_window_days(Some(0.3)), Some(1));
        assert_eq!(tavily_window_days(Some(7.0)), Some(7));
        assert_eq!(tavily_window_days(Some(6.2)), Some(7));
        assert_eq!(tavily_window_days(Some(90.0)), Some(30));
        // Clock skew (a negative interval) floors to the minimum window.
        assert_eq!(tavily_window_days(Some(-2.0)), Some(1));
    }

    #[test]
    fn start_date_is_today_minus_the_clamped_window() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 6, 18).unwrap();
        assert_eq!(tavily_start_date(today, None), None);
        assert_eq!(
            tavily_start_date(today, Some(7.0)).as_deref(),
            Some("2026-06-11")
        );
        assert_eq!(
            tavily_start_date(today, Some(0.3)).as_deref(),
            Some("2026-06-17")
        );
        assert_eq!(
            tavily_start_date(today, Some(120.0)).as_deref(),
            Some("2026-05-19")
        );
    }

    #[test]
    fn search_body_includes_start_date_only_when_present() {
        let with = search_body("fed policy", Some("2026-06-11"));
        assert_eq!(with["start_date"], serde_json::json!("2026-06-11"));
        assert_eq!(with["topic"], serde_json::json!("news"));
        assert!(with.get("days").is_none(), "the retired days field is not sent");
        let without = search_body("fed policy", None);
        assert!(
            without.get("start_date").is_none(),
            "start_date omitted when unset: {without}"
        );
    }

    #[test]
    fn run_search_round_trips_a_200_into_headlines() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"results":[{"title":"Fed holds","url":"https://www.reuters.com/markets/fed","content":"excerpt","published_date":"2026-06-05"}]}"#,
        }]);
        let source = test_source(&server.base_url);
        let headlines = source
            .run_search("fed policy", None)
            .expect("search succeeds");
        assert_eq!(server.attempts(), 1);
        assert_eq!(server.request_paths(), ["/search"]);
        assert_eq!(headlines.len(), 1);
        assert_eq!(headlines[0].title, "Fed holds");
        assert_eq!(headlines[0].source, "reuters.com");
    }

    #[test]
    fn run_search_propagates_a_non_2xx_as_fatal() {
        // A 401 is non-retryable, so the round trip stays a single sleepless
        // request.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 401,
            headers: vec![],
            body: "unauthorized",
        }]);
        let source = test_source(&server.base_url);
        let result = source.run_search("fed policy", None);
        assert_eq!(server.attempts(), 1);
        assert_eq!(server.request_paths(), ["/search"]);
        assert!(result.is_err(), "a 401 is fatal to the search");
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

        assert_eq!(headlines[1].title, "no snippet");
        assert_eq!(headlines[1].snippet, None);
    }

    fn topic_headline(q: &str) -> RawHeadline {
        RawHeadline {
            title: q.to_string(),
            url: format!("https://example.com/{q}"),
            source: "example.com".into(),
            published: None,
            snippet: None,
        }
    }

    #[test]
    fn sweep_topics_skips_a_failing_topic_and_keeps_the_rest() {
        let topics = ["alpha", "bravo", "charlie"];
        let ctx = crate::progress::RunContext::noop();
        let out = sweep_topics(&topics, &ctx, |q| {
            if q == "bravo" {
                anyhow::bail!("bravo search failed")
            }
            Ok(vec![topic_headline(q)])
        });
        let titles: Vec<&str> = out.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, vec!["alpha", "charlie"]);
    }

    #[test]
    fn sweep_topics_returns_empty_when_every_topic_fails() {
        let topics = ["alpha", "bravo"];
        let ctx = crate::progress::RunContext::noop();
        let out = sweep_topics(&topics, &ctx, |_| anyhow::bail!("rejected key"));
        assert!(out.is_empty());
    }

    #[test]
    fn sweep_topics_stops_at_cancellation_without_searching() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        use crate::progress::{NoopReporter, RunContext};

        let topics = ["alpha", "bravo"];
        let ctx = RunContext::new("t", Arc::new(NoopReporter), Arc::new(AtomicBool::new(true)));
        let mut calls = 0;
        let out = sweep_topics(&topics, &ctx, |q| {
            calls += 1;
            Ok(vec![topic_headline(q)])
        });
        assert!(out.is_empty(), "a pre-cancelled sweep gathers nothing");
        assert_eq!(calls, 0, "a pre-cancelled sweep issues no searches");
    }

    #[test]
    #[ignore = "hits the live Tavily API; set TAVILY_API_KEY"]
    fn live_search_smoke() {
        let tavily = TavilyNewsSource::from_env().expect("TAVILY_API_KEY set");
        let headlines = tavily
            .run_search("US economy inflation and the Federal Reserve", None)
            .unwrap();
        assert!(!headlines.is_empty(), "expected live Tavily results");
        assert!(headlines.iter().all(|h| !h.url.is_empty()));
    }
}
