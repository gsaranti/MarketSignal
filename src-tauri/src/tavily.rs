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
//! Degradation policy. `interpret_tavily` still turns any non-2xx into an error per
//! call (401/403 a rejected key, 429/5xx an availability problem, anything else
//! unexpected); that error surfaces on the `SearchBackend` path the Step-9 executor
//! drives, and as a `failed` tracker row on each topic. But the Step-7 `gather` sweep is
//! **per-topic fail-soft** (`sweep_topics`): a single failed topic degrades to no
//! headlines for that topic and the sweep continues — like `fmp`'s per-series skip — so
//! one bad topic (a persistent 5xx, or a malformed 2xx that retry doesn't cover) never
//! discards the other topics, nor GDELT's headlines via `CompositeNewsSource`. This is the
//! research half's fully-fail-soft posture (`pipeline::assemble_research_packet`): a
//! thinned or empty news set degrades the report rather than failing the run. (Earlier this
//! gather was loud — fail the whole sweep on any non-2xx — which made sense when a Tavily
//! failure failed the run; once research went fully fail-soft, loud-gather only turned a
//! single-topic blip into a silent total news loss, so the sweep degrades per topic now.)

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::news::{host_of, NewsSource, RawHeadline, NEWS_TOPICS};
use crate::progress::RunContext;

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
    /// Run context for the per-topic tracker rows the `gather` sweep emits. Defaults
    /// to a no-op (tests / offline smokes); the live command attaches the real one via
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
            progress: RunContext::noop(),
        })
    }

    /// Attach a live run context so the topic sweep streams one request row per Tavily
    /// call to the tracker. Without it the adapter keeps its no-op context.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Resolve the adapter from the environment, for the live smoke and any caller
    /// that bypasses the gate. The execution gate (`config::validate`) runs ahead
    /// of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().tavily_key()?)
    }

    /// Issue one search query, returning its headlines. Drives both the fixed
    /// Step-7 topic sweep (`gather`) and the Step-9 executor's arbitrary plan
    /// queries (`SearchBackend`). A transport error (Tavily unreachable) or any
    /// non-2xx response propagates as a fatal error.
    fn run_search(&self, query: &str) -> Result<Vec<RawHeadline>> {
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

/// Run the fixed Step-7 topic sweep through `search`, **per-topic fail-soft**: a topic
/// whose search errors degrades to no headlines for that topic — logged, with a `failed`
/// tracker row so the loss stays visible — and the sweep continues. One bad topic never
/// discards the rest (nor GDELT, via `CompositeNewsSource`), matching `fmp`'s per-series
/// skip and the research half's fully-fail-soft posture. Cooperative cancel breaks the
/// sweep at the next topic boundary. Extracted from `gather` so the soft-skip is
/// unit-testable without a live HTTP client.
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
        // One tracker row per actual Tavily call (the run-tracking invariant), keyed by
        // topic. The `SearchBackend::search` path issues no row here — the Step-9 executor
        // brackets those calls itself, so sharing `run_search` does not double-count.
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
    fn gather(&self) -> Result<Vec<RawHeadline>> {
        Ok(sweep_topics(NEWS_TOPICS, &self.progress, |query| {
            self.run_search(query)
        }))
    }
}

impl crate::research_executor::SearchBackend for TavilyNewsSource {
    /// The Step-9 executor drives arbitrary plan queries through the same Tavily
    /// `/search` path the topic sweep uses.
    fn search(&self, query: &str) -> Result<Vec<RawHeadline>> {
        self.run_search(query)
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
        // The middle topic's search errors; the surrounding topics must still land — one
        // bad topic does not discard the whole sweep (and so does not, via the composite,
        // discard GDELT either).
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
        // A rejected key fails every topic; the sweep degrades to no headlines rather than
        // erroring — the assembler then sees an empty news set and the run continues.
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

        // A context already cancelled: the sweep issues no searches and returns empty.
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
        let headlines = tavily.run_search("US economy inflation and the Federal Reserve").unwrap();
        assert!(!headlines.is_empty(), "expected live Tavily results");
        assert!(headlines.iter().all(|h| !h.url.is_empty()));
    }
}
