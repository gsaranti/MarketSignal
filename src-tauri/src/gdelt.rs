//! Real GDELT adapter for Step-7 news ingestion (`NewsSource`).
//!
//! GDELT's DOC 2.0 API is keyless (`docs/data-sources.md` §GDELT) — it
//! strengthens geopolitical and large-scale news-trend awareness. This adapter
//! issues a **single** combined `ArtList` query (`GDELT_QUERY`) and maps the
//! articles into provider-agnostic `RawHeadline`s. Being keyless, it sits outside
//! the execution gate (like `bls`).
//!
//! One request, not one-per-topic. GDELT burst-limits aggressively (its guidance
//! is roughly one query every few seconds), so firing a request per news category
//! back-to-back gets all but the first 429'd — verified live. Instead the news
//! categories are folded into one broad OR query and a single request, which both
//! avoids the rate limit and fits GDELT's role as the broad-trend layer better
//! than several narrow phrase queries. The query is GDELT-specific keywords rather
//! than the Tavily-style natural-language `NEWS_TOPICS`.
//!
//! Like `fmp`, the HTTP call is synchronous (`reqwest::blocking`) so the
//! `NewsSource` trait stays sync; the blocking work is offloaded via
//! `spawn_blocking` at the Tauri command seam.
//!
//! Degradation policy. GDELT is the best-effort geopolitical layer, not a gated
//! provider, so its absence must never fail the job: a failing query (transport
//! error, non-2xx, or an unparseable body) logs and degrades to no headlines
//! rather than failing the gather — the same fail-soft posture the
//! economic-release calendar uses. So `gather` never errors on GDELT's account.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::cadence::ReportCadence;
use crate::news::{host_of, NewsSource, RawHeadline};
use crate::progress::RunContext;

const GDELT_DOC_URL: &str = "https://api.gdeltproject.org/api/v2/doc/doc";

/// Per-request timeout, matching the Tavily adapter.
const GDELT_TIMEOUT: Duration = Duration::from_secs(20);

/// Articles requested for the single combined query. 250 is GDELT's `ArtList`
/// ceiling — taken in full since one query now covers every news category.
const GDELT_MAX_RECORDS: &str = "250";

/// Coverage window used on the **first report**, when there is no prior report to
/// measure an interval against — a one-week lookback keeps the geopolitical feed
/// recent and bounded. On every later run the window is sized to the actual elapsed
/// interval since the last report (see [`gdelt_timespan`]).
const GDELT_DEFAULT_TIMESPAN: &str = "1w";

/// Floor and cap (in days) on the cadence-sized GDELT window. The floor keeps a
/// same-day regeneration from requesting a window so narrow it returns almost
/// nothing; the cap holds a long gap to a month's lookback — plenty for the
/// broad-trend layer, and `DateDesc` + `GDELT_MAX_RECORDS` already surface the most
/// recent articles first. App-layer tunables, not doc-pinned.
const GDELT_WINDOW_FLOOR_DAYS: i64 = 1;
const GDELT_WINDOW_CAP_DAYS: i64 = 31;

/// Pick the GDELT `timespan` for this run from the report cadence. The first report
/// (no elapsed interval) keeps the fixed [`GDELT_DEFAULT_TIMESPAN`]; every later run
/// rounds the elapsed days *up* (so a partial day still covers its whole span) and
/// clamps to `[GDELT_WINDOW_FLOOR_DAYS, GDELT_WINDOW_CAP_DAYS]`, rendered as GDELT's
/// `"<n>d"` form. This changes only the window width, not the request count — still a
/// single bounded query, so it stays inside GDELT's burst budget.
fn gdelt_timespan(elapsed_days: Option<f64>) -> String {
    match elapsed_days {
        None => GDELT_DEFAULT_TIMESPAN.to_string(),
        Some(d) => {
            let days = (d.ceil() as i64).clamp(GDELT_WINDOW_FLOOR_DAYS, GDELT_WINDOW_CAP_DAYS);
            format!("{days}d")
        }
    }
}

/// GDELT gates on the User-Agent: requests without a descriptive one are
/// rate-limited / refused even at low volume (gdelt-doc-api#22 — "the API now
/// needs a user agent before returning any data"). The canonical Python DOC
/// client identifies itself the same way; reqwest sends none by default, so set
/// one explicitly. (This is necessary but not sufficient — GDELT also enforces a
/// ~1-request-per-5s budget with an escalating IP lockout; we stay well under it
/// by issuing a single combined query per gather, and degrade fail-soft on
/// a 429.)
const GDELT_USER_AGENT: &str =
    "MarketSignal/0.1 (market signal report; news ingestion via GDELT DOC 2.0)";

/// The single broad query covering the Step-3 news categories (politics,
/// geopolitics, China/trade, energy, earnings, AI/semiconductors, major economic
/// developments), as GDELT-specific keywords OR'd together. One query rather than
/// one per category keeps the adapter under GDELT's burst rate limit; multi-word
/// terms are quoted so they match as phrases, not ANDed tokens.
const GDELT_QUERY: &str = r#"("stock market" OR tariffs OR "oil prices" OR inflation OR semiconductors OR geopolitics OR "Federal Reserve")"#;

/// One GDELT ArtList article, trimmed to the fields a headline needs. All default
/// so an article missing an optional field still parses; `url` and `title` are
/// what the filter keys on and incomplete rows are dropped in mapping.
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

/// Parse a GDELT ArtList JSON body into raw headlines. GDELT returns an empty or
/// whitespace body when a query matches nothing — treated as no articles, not an
/// error. A non-empty body that won't parse is an error the caller soft-degrades.
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
            // Normalize GDELT's `domain` the same way Tavily's URL host is, so the
            // `source` field reads consistently across providers.
            source: host_of(&a.domain),
            title: a.title,
            url: a.url,
            published: a.seendate,
            snippet: None,
        })
        .collect())
}

/// Live GDELT adapter behind the `NewsSource` trait. Holds no key — GDELT is
/// keyless.
pub struct GdeltNewsSource {
    http: reqwest::blocking::Client,
    /// Run context for the single tracker row the gather emits. Defaults to a no-op
    /// (tests / offline smokes); the live command attaches the real one via
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

    /// Attach a live run context so the single GDELT query streams a request row to the
    /// tracker. Without it the adapter keeps its no-op context.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Run one query over the given `timespan` window, returning its headlines or an
    /// error it failed on. `gather` applies the fail-soft policy and computes the
    /// window from the run cadence; this stays honest about a failure so the caller
    /// can log it.
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
    /// Fail-soft: a failing query logs and degrades to no headlines rather than
    /// failing the gather, so GDELT (keyless, best-effort) can never fail the job.
    fn gather(&self, cadence: ReportCadence) -> Result<Vec<RawHeadline>> {
        // Cooperative cancel: skip the call entirely if a stop was already requested.
        if self.progress.is_cancelled() {
            return Ok(Vec::new());
        }
        // Size the lookback to the elapsed interval since the last report.
        let timespan = gdelt_timespan(cadence.elapsed_days());
        // One tracker row for GDELT's single combined query.
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
        // GDELT's `domain` is normalized through host_of (lowercased, www. dropped).
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
        // GDELT returns an empty body for a query that matches nothing.
        assert_eq!(headlines_from_body("").unwrap().len(), 0);
        assert_eq!(headlines_from_body("   ").unwrap().len(), 0);
        // A `{}` body (no articles field) likewise parses to nothing.
        assert_eq!(headlines_from_body("{}").unwrap().len(), 0);
    }

    #[test]
    fn non_empty_garbage_body_is_an_error() {
        // A non-empty body that isn't JSON is an error (which gather soft-degrades).
        assert!(headlines_from_body("<html>rate limited</html>").is_err());
    }

    #[test]
    fn timespan_is_sized_to_the_elapsed_interval_and_clamped() {
        // First report (no interval) keeps the fixed default.
        assert_eq!(gdelt_timespan(None), "1w");
        // A partial day rounds up to the floor rather than collapsing to nothing.
        assert_eq!(gdelt_timespan(Some(0.3)), "1d");
        // A whole interval rounds up to its day count.
        assert_eq!(gdelt_timespan(Some(7.0)), "7d");
        assert_eq!(gdelt_timespan(Some(6.2)), "7d");
        // A long gap is held to the cap, not an unbounded month-plus window.
        assert_eq!(gdelt_timespan(Some(120.0)), "31d");
        // Clock skew (a negative interval) floors to the minimum window.
        assert_eq!(gdelt_timespan(Some(-3.0)), "1d");
    }

    #[test]
    #[ignore = "hits the live GDELT API (keyless)"]
    fn live_gather_smoke() {
        let gdelt = GdeltNewsSource::new().unwrap();
        // gather is fail-soft and never errors — so a vacuous `all()` on an empty
        // result would silently pass even when GDELT is rate-limited to nothing
        // (the failure mode that motivated the single-query consolidation). Require
        // a non-empty result so that regression surfaces here.
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
