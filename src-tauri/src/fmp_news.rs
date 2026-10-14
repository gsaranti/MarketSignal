//! FMP Articles adapter for news ingestion (`NewsSource`;
//! `docs/data-sources.md` §FMP Articles): FMP's in-house, ticker-tagged
//! editorial feed, the one FMP news surface on the free tier (the third-party
//! `news/*` feeds answer 402). One bounded page per gather. Fail-soft: a
//! failing gather degrades to no headlines so a flaky supplement never costs
//! the Tavily headlines via `CompositeNewsSource`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::cadence::ReportCadence;
use crate::news::{host_of, NewsSource, RawHeadline};
use crate::progress::RunContext;

const FMP_NEWS_BASE: &str = "https://financialmodelingprep.com/stable";
const FMP_ARTICLES_PATH: &str = "/fmp-articles";

const FMP_NEWS_TIMEOUT: Duration = Duration::from_secs(20);

/// About one Tavily topic's worth, so the supplement widens the funnel without
/// meaningfully growing the headline filter's token cost.
const ARTICLES_LIMIT: &str = "20";

/// Characters. FMP's `content` is a full editorial article; the filter model
/// only needs an excerpt comparable to Tavily's.
const SNIPPET_MAX_CHARS: usize = 400;

/// All fields default so an article missing one still parses. `tickers` is
/// FMP's exchange-prefixed tag (e.g. "NYSE:ELF"); `content` is HTML.
#[derive(Debug, Deserialize)]
struct FmpArticleRaw {
    #[serde(default)]
    title: String,
    #[serde(default)]
    link: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    content: String,
    #[serde(default)]
    tickers: String,
}

/// 402 gets its own message since it would mean the feed left the free tier.
fn interpret_fmp_articles(status: u16, body: &str) -> Result<Vec<FmpArticleRaw>> {
    match status {
        200..=299 => {
            serde_json::from_str(body).context("FMP articles returned an unparseable 2xx body")
        }
        401 | 403 => bail!("FMP rejected the key (HTTP {status})"),
        402 => bail!("FMP articles is premium-gated (HTTP 402) — it may have left the free tier"),
        429 | 500..=599 => bail!("FMP articles is unavailable (HTTP {status})"),
        _ => bail!("FMP articles returned an unexpected response (HTTP {status})"),
    }
}

/// Drops any article missing a link or title. The snippet is prefixed with the
/// ticker tag so the filter model sees the company association `RawHeadline`
/// has no field for.
fn headlines_from_articles(articles: Vec<FmpArticleRaw>) -> Vec<RawHeadline> {
    articles
        .into_iter()
        .filter(|a| !a.link.trim().is_empty() && !a.title.trim().is_empty())
        .map(|a| RawHeadline {
            source: host_of(&a.link),
            title: a.title,
            url: a.link,
            published: a.date.filter(|d| !d.trim().is_empty()),
            snippet: compose_snippet(&a.tickers, &a.content),
        })
        .collect()
}

/// `[tickers] text…`, capped at [`SNIPPET_MAX_CHARS`]. Both parts empty yields
/// `None`, not an empty string.
fn compose_snippet(tickers: &str, content_html: &str) -> Option<String> {
    let text = strip_html(content_html);
    let tickers = tickers.trim();
    let snippet = match (tickers.is_empty(), text.is_empty()) {
        (true, true) => return None,
        (true, false) => text,
        (false, true) => format!("[{tickers}]"),
        (false, false) => format!("[{tickers}] {text}"),
    };
    Some(truncate_chars(&snippet, SNIPPET_MAX_CHARS))
}

/// Each tag becomes a space so adjacent list items don't fuse. `&amp;` is
/// decoded last so a double-escaped entity stays literal.
fn strip_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `max` counts characters, not bytes: the editorial text carries multi-byte
/// punctuation.
fn truncate_chars(s: &str, max: usize) -> String {
    let (head, cut) = crate::data_sources::cap_chars(s, max);
    if cut {
        format!("{head}…")
    } else {
        head
    }
}

/// Fail-soft: a failing fetch degrades to no headlines, with a `failed`
/// tracker row so the loss stays visible. Extracted from `gather` so the
/// soft-skip is unit-testable without a live HTTP client.
fn gather_fail_soft(
    progress: &RunContext,
    fetch: impl FnOnce() -> Result<Vec<RawHeadline>>,
) -> Vec<RawHeadline> {
    if progress.is_cancelled() {
        return Vec::new();
    }
    progress.request_started("FMP", "news", "fmp-articles", "FMP Articles feed");
    match fetch() {
        Ok(headlines) => {
            progress.request_finished(
                "FMP",
                "news",
                "fmp-articles",
                "FMP Articles feed",
                "ok",
                None,
            );
            headlines
        }
        Err(e) => {
            eprintln!("FMP articles gather degraded to empty: {e}");
            progress.request_finished(
                "FMP",
                "news",
                "fmp-articles",
                "FMP Articles feed",
                "failed",
                Some(e.to_string()),
            );
            Vec::new()
        }
    }
}

/// Reuses the FMP key the execution gate already requires for market data.
pub struct FmpNewsSource {
    api_key: String,
    http: reqwest::blocking::Client,
    /// Defaults to [`FMP_NEWS_BASE`]; a test overrides it via
    /// [`FmpNewsSource::with_base_url`].
    base_url: String,
    /// Defaults to a no-op; the live command attaches the real one via
    /// [`FmpNewsSource::with_context`].
    progress: Arc<RunContext>,
}

impl FmpNewsSource {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(FMP_NEWS_TIMEOUT)
            .build()
            .context("building the FMP articles HTTP client")?;
        Ok(Self {
            api_key,
            http,
            base_url: FMP_NEWS_BASE.to_string(),
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
        Self::new(crate::config::AppConfig::from_env().fmp_key()?)
    }

    /// Stays honest about a failure; `gather` applies the fail-soft policy.
    /// Retries ride the shared FMP policy (`fmp::FMP_RETRY`), the run's cancel
    /// flag aborting a backoff mid-sleep.
    fn fetch_articles(&self) -> Result<Vec<RawHeadline>> {
        let url = format!("{}{FMP_ARTICLES_PATH}", self.base_url);
        let abort = || self.progress.is_cancelled();
        let (status, body) = crate::http_retry::send_with_retry_policy(
            "FMP",
            &crate::fmp::FMP_RETRY,
            Some(&abort),
            || {
                self.http.get(&url).query(&[
                    ("page", "0"),
                    ("limit", ARTICLES_LIMIT),
                    ("apikey", self.api_key.as_str()),
                ])
            },
        )?;
        Ok(headlines_from_articles(interpret_fmp_articles(
            status, &body,
        )?))
    }
}

impl NewsSource for FmpNewsSource {
    /// The cadence is ignored: a single bounded editorial page has no recency
    /// window to size.
    fn gather(&self, _cadence: ReportCadence) -> Result<Vec<RawHeadline>> {
        Ok(gather_fail_soft(&self.progress, || self.fetch_articles()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    #[test]
    fn interpret_fmp_articles_covers_the_status_matrix() {
        let body = r#"[{"title":"t","link":"https://x.com/a","content":"c"}]"#;
        assert_eq!(interpret_fmp_articles(200, body).unwrap().len(), 1);
        assert!(interpret_fmp_articles(200, "[]").unwrap().is_empty());

        for status in [401, 403, 402, 429, 500, 503, 400, 404] {
            assert!(
                interpret_fmp_articles(status, "").is_err(),
                "HTTP {status} should be an error"
            );
        }
        // 402 names the premium gate so a tier regression reads off the tracker row.
        let premium = interpret_fmp_articles(402, "").unwrap_err().to_string();
        assert!(
            premium.contains("premium"),
            "402 message names the gate: {premium}"
        );
        assert!(interpret_fmp_articles(200, "not json").is_err());
    }

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----

    fn test_source(base_url: &str) -> FmpNewsSource {
        FmpNewsSource::new("test-key".to_string())
            .expect("build adapter")
            .with_base_url(base_url)
    }

    #[test]
    fn fetch_articles_round_trips_a_200_into_headlines() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"[{"title":"E.l.f. Beauty Stock","date":"2026-06-11 21:06:55","content":"<p>news</p>","tickers":"NYSE:ELF","link":"https://financialmodelingprep.com/market-news/elf-stock"}]"#,
        }]);
        let source = test_source(&server.base_url);
        let headlines = source.fetch_articles().expect("articles fetch succeeds");
        assert_eq!(server.attempts(), 1);
        assert_eq!(server.request_paths(), ["/fmp-articles"]);
        assert_eq!(headlines.len(), 1);
        assert_eq!(headlines[0].title, "E.l.f. Beauty Stock");
        assert_eq!(headlines[0].source, "financialmodelingprep.com");
    }

    #[test]
    fn fetch_articles_propagates_a_non_2xx_as_error() {
        // A 402 is non-retryable, so the round trip stays a single sleepless
        // request.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 402,
            headers: vec![],
            body: "premium",
        }]);
        let source = test_source(&server.base_url);
        let result = source.fetch_articles();
        assert_eq!(server.attempts(), 1);
        assert_eq!(server.request_paths(), ["/fmp-articles"]);
        assert!(result.is_err(), "a 402 is an error for fetch_articles");
    }

    #[test]
    fn headlines_from_articles_maps_and_drops_incomplete_rows() {
        let body = r#"[
            {"title":"E.l.f. Beauty Stock Performance","date":"2026-06-11 21:06:55",
             "content":"<ul><li>Bernstein initiated a &quot;Market Perform&quot; rating.</li></ul>",
             "tickers":"NYSE:ELF","image":"https://portal.fmp.com/x.jpeg",
             "link":"https://financialmodelingprep.com/market-news/elf-stock","author":"A","site":"Financial Modeling Prep"},
            {"title":"no link","link":"","content":"x"},
            {"title":"","link":"https://x.com/empty-title","content":"x"},
            {"title":"bare","link":"https://financialmodelingprep.com/market-news/bare","date":"  "}
        ]"#;
        let articles = interpret_fmp_articles(200, body).unwrap();
        let headlines = headlines_from_articles(articles);
        assert_eq!(headlines.len(), 2, "the two incomplete rows are dropped");

        assert_eq!(headlines[0].title, "E.l.f. Beauty Stock Performance");
        assert_eq!(
            headlines[0].source, "financialmodelingprep.com",
            "source is the link's host"
        );
        assert_eq!(
            headlines[0].published.as_deref(),
            Some("2026-06-11 21:06:55")
        );
        assert_eq!(
            headlines[0].snippet.as_deref(),
            Some("[NYSE:ELF] Bernstein initiated a \"Market Perform\" rating.")
        );

        assert_eq!(headlines[1].title, "bare");
        assert_eq!(headlines[1].snippet, None);
        assert_eq!(headlines[1].published, None);
    }

    #[test]
    fn strip_html_drops_tags_decodes_entities_and_collapses_whitespace() {
        let html = "<h1 class=\"x\">Top</h1>\n<ul>\n  <li><strong>13%</strong> decline</li>\n  <li>M&amp;A talk</li>\n</ul>";
        assert_eq!(strip_html(html), "Top 13% decline M&A talk");
        // A double-escaped entity stays literal.
        assert_eq!(strip_html("a &amp;lt; b"), "a &lt; b");
        assert_eq!(strip_html("  plain   text  "), "plain text");
    }

    #[test]
    fn compose_snippet_bounds_and_prefixes() {
        let long = "x".repeat(SNIPPET_MAX_CHARS * 2);
        let snippet = compose_snippet("", &long).unwrap();
        assert_eq!(snippet.chars().count(), SNIPPET_MAX_CHARS + 1);
        assert!(snippet.ends_with('…'));
        let exact = "y".repeat(SNIPPET_MAX_CHARS);
        assert_eq!(compose_snippet("", &exact).unwrap(), exact);
        assert_eq!(compose_snippet("NYSE:AD", "").as_deref(), Some("[NYSE:AD]"));
        assert_eq!(compose_snippet("", "  "), None);
    }

    #[test]
    fn gather_fail_soft_returns_headlines_and_an_ok_row() {
        use crate::progress::{ProgressEvent, RecordingReporter, RunContext};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("t", rec.clone(), Arc::new(AtomicBool::new(false)));
        let out = gather_fail_soft(&ctx, || {
            Ok(vec![RawHeadline {
                title: "t".into(),
                url: "https://x.com/a".into(),
                source: "x.com".into(),
                published: None,
                snippet: None,
            }])
        });
        assert_eq!(out.len(), 1);
        let messages = rec.messages();
        assert_eq!(messages.len(), 2, "one started + one finished row");
        match &messages[1].event {
            ProgressEvent::RequestFinished {
                provider,
                group,
                status,
                ..
            } => {
                assert_eq!(provider, "FMP");
                assert_eq!(group, "news", "rows bucket under the tracker's news group");
                assert_eq!(status, "ok");
            }
            other => panic!("expected RequestFinished, got {other:?}"),
        }
    }

    #[test]
    fn gather_fail_soft_degrades_a_failure_to_empty_with_a_failed_row() {
        use crate::progress::{ProgressEvent, RecordingReporter, RunContext};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("t", rec.clone(), Arc::new(AtomicBool::new(false)));
        let out = gather_fail_soft(&ctx, || anyhow::bail!("feed down"));
        assert!(out.is_empty(), "a failing fetch degrades to no headlines");
        let messages = rec.messages();
        match &messages[1].event {
            ProgressEvent::RequestFinished { status, detail, .. } => {
                assert_eq!(status, "failed");
                assert_eq!(detail.as_deref(), Some("feed down"));
            }
            other => panic!("expected RequestFinished, got {other:?}"),
        }
    }

    #[test]
    fn gather_fail_soft_skips_the_fetch_when_cancelled() {
        use crate::progress::{NoopReporter, RunContext};
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let ctx = RunContext::new("t", Arc::new(NoopReporter), Arc::new(AtomicBool::new(true)));
        let mut called = false;
        let out = gather_fail_soft(&ctx, || {
            called = true;
            Ok(Vec::new())
        });
        assert!(out.is_empty());
        assert!(!called, "a pre-cancelled gather issues no fetch");
    }

    #[test]
    #[ignore = "hits the live FMP API; set FMP_API_KEY (1 request)"]
    fn fmp_news_smoke() {
        let src = FmpNewsSource::from_env().expect("FMP_API_KEY set");
        // gather never errors, so a bare success check is vacuous; require a
        // non-empty result so a tier regression (402) surfaces here.
        let headlines = src.gather(ReportCadence::from_elapsed(None)).unwrap();
        eprintln!(
            "FMP articles live gather returned {} headlines",
            headlines.len()
        );
        assert!(
            !headlines.is_empty(),
            "FMP articles returned no headlines — premium-gated, or the feed moved"
        );
        assert!(headlines
            .iter()
            .all(|h| !h.title.is_empty() && !h.url.is_empty()));
        assert!(
            headlines.iter().any(|h| h.snippet.is_some()),
            "expected at least one article with editorial content"
        );
    }
}
