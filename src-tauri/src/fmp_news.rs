//! Real FMP Articles adapter for Step-7 news ingestion (`NewsSource`).
//!
//! FMP Articles is Financial Modeling Prep's in-house, ticker-tagged editorial
//! feed (`docs/data-sources.md` §FMP Articles) — the one FMP news surface on the
//! free tier (verified live 2026-06-11 by `fmp::tests::fmp_news_probe`: HTTP 200
//! with `page`/`limit` honored, while the third-party `news/*` feeds all 402).
//! It supplements Tavily's topic sweep and GDELT's geopolitical query with
//! company-level market headlines, reusing the FMP key the baseline scan already
//! requires. This adapter issues a **single** bounded page per gather and maps
//! the articles into provider-agnostic `RawHeadline`s.
//!
//! Like `fmp`, the HTTP call is synchronous (`reqwest::blocking`) so the
//! `NewsSource` trait stays sync; the blocking work is offloaded via
//! `spawn_blocking` at the Tauri command seam.
//!
//! Degradation policy. FMP Articles is a best-effort supplementary layer, like
//! GDELT: a failing gather (transport error, non-2xx, or an unparseable body)
//! logs and degrades to no headlines rather than failing the gather — a flaky
//! hedge source must never cost the Tavily headlines via `CompositeNewsSource`'s
//! error propagation. So `gather` never errors on this adapter's account.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::news::{host_of, NewsSource, RawHeadline};
use crate::progress::RunContext;

/// Base URL for FMP's stable API. The single endpoint path below is joined onto it in
/// [`FmpNewsSource::fetch_articles`]; a test redirects the adapter at a localhost mock
/// via [`FmpNewsSource::with_base_url`], so the wire path runs offline.
const FMP_NEWS_BASE: &str = "https://financialmodelingprep.com/stable";
const FMP_ARTICLES_PATH: &str = "/fmp-articles";

/// Per-request timeout, matching the Tavily and GDELT adapters.
const FMP_NEWS_TIMEOUT: Duration = Duration::from_secs(20);

/// Articles requested for the single bounded page — about one Tavily topic's
/// worth, so the supplementary feed widens the funnel without meaningfully
/// growing the headline-filter's token cost.
const ARTICLES_LIMIT: &str = "20";

/// Snippet ceiling, in characters. FMP's `content` is a full editorial article;
/// the filter model only needs an excerpt comparable to Tavily's.
const SNIPPET_MAX_CHARS: usize = 400;

/// One FMP article, trimmed to the fields a headline needs. All default so an
/// article missing a field still parses; `link` and `title` are what the filter
/// keys on and incomplete rows are dropped in mapping. `tickers` is FMP's
/// exchange-prefixed tag (e.g. "NYSE:ELF"); `content` is HTML.
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

/// Interpret an FMP articles HTTP response by status, parsing the bare-array body
/// only on a 2xx. Any non-2xx is an error the caller soft-degrades; 402 gets its
/// own message since it would mean the feed left the free tier.
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

/// Shape parsed articles into raw headlines, dropping any article missing a link
/// or title (nothing the filter could key on). The publisher `source` is the
/// link's host (FMP's own market-news pages); the snippet is the HTML `content`
/// stripped to text and bounded, prefixed with the article's ticker tag so the
/// filter model sees the company association `RawHeadline` has no field for.
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

/// Build the bounded snippet: `[tickers] text…` with the HTML stripped, ticker
/// tag prefixed when present, and the whole thing capped at
/// [`SNIPPET_MAX_CHARS`]. Both parts empty yields `None`, not an empty string.
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

/// Strip HTML down to its text: tags dropped (each replaced by a space so
/// adjacent list items don't fuse), the handful of entities FMP's editorial
/// markup uses decoded, and whitespace collapsed. `&amp;` is decoded last so a
/// double-escaped entity stays literal rather than double-decoding.
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

/// Truncate to `max` characters (not bytes — the editorial text can carry
/// multi-byte punctuation), appending an ellipsis when anything was cut.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Run the single-page gather **fail-soft**: a failing fetch logs and degrades to
/// no headlines — with a `failed` tracker row so the loss stays visible — rather
/// than erroring, and a cooperative cancel skips the call entirely. Extracted
/// from `gather` so the soft-skip and its tracker rows are unit-testable without
/// a live HTTP client, like `tavily::sweep_topics`.
fn gather_fail_soft(
    progress: &RunContext,
    fetch: impl FnOnce() -> Result<Vec<RawHeadline>>,
) -> Vec<RawHeadline> {
    if progress.is_cancelled() {
        return Vec::new();
    }
    // One tracker row for the single bounded page (the run-tracking invariant:
    // one row per actual HTTP call).
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

/// Live FMP Articles adapter behind the `NewsSource` trait. Reuses the FMP key
/// the execution gate already requires for market data.
pub struct FmpNewsSource {
    api_key: String,
    http: reqwest::blocking::Client,
    /// API origin the endpoint path is joined onto. Defaults to [`FMP_NEWS_BASE`]; an
    /// offline round-trip test overrides it via [`FmpNewsSource::with_base_url`].
    base_url: String,
    /// Run context for the single tracker row the gather emits. Defaults to a no-op
    /// (tests / offline smokes); the live command attaches the real one via
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

    /// Redirect the adapter at an alternate API origin (a localhost mock) so the wire
    /// path runs offline. Test-only; a trailing slash is trimmed so the joined path's
    /// leading slash doesn't double up.
    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// Attach a live run context so the single articles fetch streams a request row
    /// to the tracker. Without it the adapter keeps its no-op context.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Resolve the adapter from the environment, for the live smoke and any caller
    /// that bypasses the gate. The execution gate (`config::validate`) runs ahead
    /// of this in the command path.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().fmp_key()?)
    }

    /// Fetch the single bounded page, returning its headlines or the error it
    /// failed on. `gather` applies the fail-soft policy; this stays honest about a
    /// failure so the caller can log it. Retries ride the shared FMP backoff.
    fn fetch_articles(&self) -> Result<Vec<RawHeadline>> {
        let url = format!("{}{FMP_ARTICLES_PATH}", self.base_url);
        let (status, body) = crate::http_retry::send_with_retry("FMP", || {
            self.http.get(&url).query(&[
                ("page", "0"),
                ("limit", ARTICLES_LIMIT),
                ("apikey", self.api_key.as_str()),
            ])
        })?;
        Ok(headlines_from_articles(interpret_fmp_articles(
            status, &body,
        )?))
    }
}

impl NewsSource for FmpNewsSource {
    /// Fail-soft: a failing fetch logs and degrades to no headlines rather than
    /// failing the gather, so this best-effort supplement can never fail the job.
    fn gather(&self) -> Result<Vec<RawHeadline>> {
        Ok(gather_fail_soft(&self.progress, || self.fetch_articles()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    #[test]
    fn interpret_fmp_articles_covers_the_status_matrix() {
        // A 2xx bare-array body parses into articles.
        let body = r#"[{"title":"t","link":"https://x.com/a","content":"c"}]"#;
        assert_eq!(interpret_fmp_articles(200, body).unwrap().len(), 1);
        // A 2xx empty array parses to nothing.
        assert!(interpret_fmp_articles(200, "[]").unwrap().is_empty());

        // Auth / premium / availability / unexpected statuses are all errors
        // (which gather soft-degrades).
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
        // A 2xx that isn't valid JSON is a contract violation -> error.
        assert!(interpret_fmp_articles(200, "not json").is_err());
    }

    // ---- Offline round trip: adapter -> retry -> interpret -> domain output ----
    //
    // The matrix above pins `interpret_fmp_articles` as a pure function; these drive the
    // whole `fetch_articles` -> `send_with_retry` -> `interpret_fmp_articles` ->
    // `headlines_from_articles` path against a localhost mock (`crate::test_http`). A
    // non-retryable error status keeps the round trip a single sleepless request (retry
    // mechanics live in `http_retry`).

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
        // A 402 premium gate is non-retryable and an error here; `gather` soft-degrades it
        // to empty (covered in `gather_fail_soft`). A 402 also keeps the round trip a
        // single sleepless request.
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
        // Snippet: ticker tag prefixed, HTML stripped, entities decoded.
        assert_eq!(
            headlines[0].snippet.as_deref(),
            Some("[NYSE:ELF] Bernstein initiated a \"Market Perform\" rating.")
        );

        // No content, no tickers, blank date -> None fields rather than empty strings.
        assert_eq!(headlines[1].title, "bare");
        assert_eq!(headlines[1].snippet, None);
        assert_eq!(headlines[1].published, None);
    }

    #[test]
    fn strip_html_drops_tags_decodes_entities_and_collapses_whitespace() {
        let html = "<h1 class=\"x\">Top</h1>\n<ul>\n  <li><strong>13%</strong> decline</li>\n  <li>M&amp;A talk</li>\n</ul>";
        assert_eq!(strip_html(html), "Top 13% decline M&A talk");
        // A double-escaped entity stays literal (amp decoded last).
        assert_eq!(strip_html("a &amp;lt; b"), "a &lt; b");
        // No HTML at all passes through with whitespace collapsed.
        assert_eq!(strip_html("  plain   text  "), "plain text");
    }

    #[test]
    fn compose_snippet_bounds_and_prefixes() {
        // The cap counts characters, not bytes, and cut text gains an ellipsis.
        let long = "x".repeat(SNIPPET_MAX_CHARS * 2);
        let snippet = compose_snippet("", &long).unwrap();
        assert_eq!(snippet.chars().count(), SNIPPET_MAX_CHARS + 1);
        assert!(snippet.ends_with('…'));
        // Exactly-at-cap text is untouched.
        let exact = "y".repeat(SNIPPET_MAX_CHARS);
        assert_eq!(compose_snippet("", &exact).unwrap(), exact);
        // A ticker tag with no content still surfaces the association.
        assert_eq!(compose_snippet("NYSE:AD", "").as_deref(), Some("[NYSE:AD]"));
        // Both empty -> no snippet at all.
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

        // A context already cancelled: no fetch, no rows, no headlines.
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
        // gather is fail-soft and never errors, so a bare success check is vacuous —
        // require a non-empty result so a tier regression (402) or feed change
        // surfaces here rather than as a silently thinner funnel.
        let headlines = src.gather().unwrap();
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
