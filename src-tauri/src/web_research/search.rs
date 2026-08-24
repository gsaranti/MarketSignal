//! The web tool's search backend: self-hosted SearXNG primary, Tavily
//! fallback (`docs/web-research.md §Search backend: SearXNG`, §Tavily
//! fallback).
//!
//! SearXNG is queried over its JSON API on the configured (loopback)
//! endpoint — the one deliberate loopback target in the web tool, an
//! app-configured service address rather than a model-chosen fetch, so it is
//! exempt from the fetch guard's loopback block. It sits **off the execution
//! gate**: unreachable means a degraded run behind a pre-run notice, never a
//! block. When SearXNG can't serve (unreachable, misconfigured — an HTTP 403
//! is the JSON-output-disabled signature — or empty), the tool falls back to
//! the metered Tavily key where one is configured.
//!
//! Rank-time policy lives here too: denied hosts are dropped at the search
//! filter, and near-duplicate / same-origin hits collapse so syndication
//! can't inflate apparent corroboration (five outlets reprinting one wire
//! story are one source, not five).

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::registry::{self, SourcePolicy};

/// Per-query timeout — SearXNG fans out to public engines, so it is slower
/// than a single API; 20s covers the observed fan-out without parking the
/// research loop.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Hits kept per query after filtering — the loop fetches a subset, so a
/// deeper tail only burns context.
const MAX_HITS: usize = 12;

/// One search hit, provider-agnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub host: String,
    pub snippet: Option<String>,
    pub published: Option<String>,
    /// The origin's evidence tier (registry / default heuristic) — carried so
    /// rank-time collapse can keep the highest-tier origin of a syndicated set.
    pub tier: u8,
}

/// The injectable search seam the research runner drives; implementations are
/// the live [`FallbackSearch`] and test scripts.
pub trait WebSearch: Send + Sync {
    fn search(&self, query: &str) -> Result<Vec<SearchHit>>;
}

/// How a `FallbackSearch` answered — recorded by the runner so a degraded
/// (Tavily) or failed search stays visible in the audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchRoute {
    Searxng,
    TavilyFallback,
}

// ---------------------------------------------------------------------------
// SearXNG
// ---------------------------------------------------------------------------

/// One SearXNG JSON result, trimmed to what a hit needs. Field names follow
/// the JSON API (`publishedDate` rides camelCase on the wire).
#[derive(Debug, Deserialize)]
struct SearxngResultRaw {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: String,
    #[serde(default, rename = "publishedDate")]
    published_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearxngResponse {
    #[serde(default)]
    results: Vec<SearxngResultRaw>,
}

/// Interpret a SearXNG HTTP response by status. The 403 arm names the
/// JSON-output-disabled misconfiguration explicitly — it is the shipped
/// `settings.yml`'s whole reason to exist — so the Settings connection row
/// and the pre-run notice can say what is actually wrong.
fn interpret_searxng(status: u16, body: &str) -> Result<SearxngResponse> {
    match status {
        200..=299 => {
            serde_json::from_str(body).context("SearXNG returned an unparseable 2xx body")
        }
        403 => bail!(
            "SearXNG rejected the JSON request (HTTP 403) — the instance is likely running \
             without JSON output enabled; use the shipped settings.yml"
        ),
        429 => bail!("SearXNG rate-limited the request (HTTP 429) — is the bot limiter disabled?"),
        _ => bail!("SearXNG returned HTTP {status}"),
    }
}

/// The local SearXNG instance over its JSON API.
pub struct SearxngClient {
    base_url: String,
    http: reqwest::blocking::Client,
}

impl SearxngClient {
    pub fn new(endpoint: &str) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(SEARCH_TIMEOUT)
            .build()
            .context("building the SearXNG HTTP client")?;
        Ok(Self {
            base_url: endpoint.trim().trim_end_matches('/').to_string(),
            http,
        })
    }

    /// One JSON search. Errors carry the misconfiguration diagnosis where the
    /// status makes one legible.
    pub fn search_raw(&self, query: &str) -> Result<Vec<SearchHit>> {
        let url = format!("{}/search", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[("q", query), ("format", "json")])
            .send()
            .with_context(|| format!("SearXNG unreachable at {}", self.base_url))?;
        let status = resp.status().as_u16();
        let body = resp.text().context("reading the SearXNG response body")?;
        let parsed = interpret_searxng(status, &body)?;
        Ok(parsed
            .results
            .into_iter()
            .filter(|r| !r.url.trim().is_empty() && !r.title.trim().is_empty())
            .map(|r| {
                let host = host_of(&r.url);
                let tier = tier_of(&host);
                SearchHit {
                    title: r.title,
                    url: r.url,
                    host,
                    snippet: if r.content.trim().is_empty() {
                        None
                    } else {
                        Some(r.content)
                    },
                    published: r.published_date,
                    tier,
                }
            })
            .collect())
    }

    /// The connection probe the Settings row and the pre-run notice read: a
    /// real JSON query, so the JSON-disabled 403 misconfiguration is caught,
    /// not just TCP reachability. Same mechanism class as the model daemon's
    /// health check — a bounded HTTP call, never on the execution gate.
    ///
    /// Serving means **returning usable results**, matching the search path's
    /// own definition — the same rank-time filter runs before the judgment, so
    /// a probe answered only by denied hosts or malformed entries reads
    /// degraded exactly as the run would treat it: an instance that responds
    /// but serves nothing usable routes every actual search to the metered
    /// Tavily fallback, and the probe must surface that rather than silently
    /// consenting to the spend.
    pub fn health_check(&self) -> Result<()> {
        let hits = filter_and_collapse(self.search_raw("financial market news")?);
        if hits.is_empty() {
            bail!(
                "SearXNG responded but returned no usable results for the probe query — its \
                 engines may be failing or disabled, so searches would silently fall back to \
                 Tavily"
            );
        }
        Ok(())
    }
}

/// The host of a URL (normalized), or an empty string when unparseable.
fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(registry::normalize_host))
        .unwrap_or_default()
}

/// The evidence tier of a host (deny handled separately at the filter).
fn tier_of(host: &str) -> u8 {
    match registry::assess(host) {
        SourcePolicy::Graded(e) => e.tier,
        SourcePolicy::Deny(_) => u8::MAX,
    }
}

// ---------------------------------------------------------------------------
// Rank-time filtering
// ---------------------------------------------------------------------------

/// Normalize a title for the syndication collapse: lowercased, alphanumeric
/// runs only — five outlets reprinting one wire story share this key.
fn title_key(title: &str) -> String {
    let mut key = String::with_capacity(title.len());
    let mut last_was_gap = true;
    for c in title.chars() {
        if c.is_alphanumeric() {
            key.extend(c.to_lowercase());
            last_was_gap = false;
        } else if !last_was_gap {
            key.push(' ');
            last_was_gap = true;
        }
    }
    key.trim().to_string()
}

/// Apply the search-filter policy: drop denied hosts, dedup exact URLs,
/// collapse syndicated near-duplicates by title (keeping the highest-tier —
/// lowest-numbered — origin), and cap the tail.
pub fn filter_and_collapse(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    use std::collections::HashMap;
    let mut kept: Vec<SearchHit> = Vec::new();
    let mut by_title: HashMap<String, usize> = HashMap::new();
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();

    for hit in hits {
        if hit.host.is_empty() {
            continue;
        }
        if matches!(registry::assess(&hit.host), SourcePolicy::Deny(_)) {
            continue;
        }
        let url_key = hit.url.trim_end_matches('/').to_string();
        if !seen_urls.insert(url_key) {
            continue;
        }
        let key = title_key(&hit.title);
        if key.is_empty() {
            continue;
        }
        match by_title.get(&key) {
            Some(&i) => {
                // Same story from another outlet: one origin, not two — keep
                // the higher-tier copy.
                if hit.tier < kept[i].tier {
                    kept[i] = hit;
                }
            }
            None => {
                by_title.insert(key, kept.len());
                kept.push(hit);
            }
        }
    }
    kept.truncate(MAX_HITS);
    kept
}

// ---------------------------------------------------------------------------
// The fallback composite
// ---------------------------------------------------------------------------

/// SearXNG-primary search with the Tavily fallback
/// (`docs/web-research.md §Tavily fallback`): the fallback engages when the
/// local instance can't serve — unreachable, misconfigured, or returning
/// nothing. Both providers' hits pass the same rank-time filter.
pub struct FallbackSearch {
    searxng: Option<SearxngClient>,
    /// The report pipeline's Tavily adapter behind its `SearchBackend` trait —
    /// reused rather than re-implemented; `None` when no Tavily key is
    /// configured (SearXNG-only, fail-soft).
    tavily: Option<Box<dyn crate::research_executor::SearchBackend + Send + Sync>>,
}

impl FallbackSearch {
    pub fn new(
        searxng: Option<SearxngClient>,
        tavily: Option<Box<dyn crate::research_executor::SearchBackend + Send + Sync>>,
    ) -> Self {
        Self { searxng, tavily }
    }

    /// Search with the route taken, so the runner can record degraded mode.
    /// "Returning nothing" is judged **after** the rank-time filter: a result
    /// set that survives only as denied or malformed entries is no more
    /// servable than a raw empty one, so it falls back the same way.
    pub fn search_routed(&self, query: &str) -> Result<(Vec<SearchHit>, SearchRoute)> {
        let primary_err = match &self.searxng {
            Some(client) => match client.search_raw(query) {
                Ok(hits) => {
                    let filtered = filter_and_collapse(hits);
                    if !filtered.is_empty() {
                        return Ok((filtered, SearchRoute::Searxng));
                    }
                    anyhow::anyhow!("SearXNG returned no usable results")
                }
                Err(e) => e,
            },
            None => anyhow::anyhow!("no SearXNG endpoint configured"),
        };
        match &self.tavily {
            Some(backend) => {
                let headlines = backend
                    .search(query)
                    .with_context(|| format!("Tavily fallback after: {primary_err}"))?;
                let hits = headlines
                    .into_iter()
                    .map(|h| {
                        let host = registry::normalize_host(&h.source);
                        let tier = tier_of(&host);
                        SearchHit {
                            title: h.title,
                            url: h.url,
                            host,
                            snippet: h.snippet,
                            published: h.published,
                            tier,
                        }
                    })
                    .collect();
                Ok((filter_and_collapse(hits), SearchRoute::TavilyFallback))
            }
            None => Err(primary_err.context("search degraded: SearXNG failed and no Tavily fallback is configured")),
        }
    }
}

impl WebSearch for FallbackSearch {
    fn search(&self, query: &str) -> Result<Vec<SearchHit>> {
        self.search_routed(query).map(|(hits, _)| hits)
    }
}

// ---------------------------------------------------------------------------
// Pre-run preflight
// ---------------------------------------------------------------------------

/// The pre-run web-research read (`docs/web-research.md §Tavily fallback` —
/// the pre-run notice; `docs/interface.md §Pre-run web-research notice`).
/// Never a gate: the frontend shows a confirm-and-proceed notice on a
/// degraded state, flagged *not recommended* when no Tavily fallback exists
/// either, and the engine-only paths never ask.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WebResearchPreflight {
    /// `ok` | `unreachable` | `not_configured`.
    pub status: String,
    /// The failure diagnosis when unreachable (e.g. the JSON-disabled 403).
    pub detail: Option<String>,
    /// Whether the metered Tavily fallback is configured.
    pub tavily_fallback: bool,
    /// True whenever SearXNG cannot serve — the notice trigger.
    pub degraded: bool,
}

/// Probe the configured SearXNG endpoint for the pre-run notice and the
/// Settings connection row. One bounded JSON query; no state change.
pub fn preflight(endpoint: Option<&str>, tavily_configured: bool) -> WebResearchPreflight {
    let (status, detail) = match endpoint.map(str::trim).filter(|s| !s.is_empty()) {
        None => ("not_configured", None),
        Some(endpoint) => match SearxngClient::new(endpoint).and_then(|c| c.health_check()) {
            Ok(()) => ("ok", None),
            Err(e) => ("unreachable", Some(format!("{e:#}"))),
        },
    };
    WebResearchPreflight {
        degraded: status != "ok",
        status: status.to_string(),
        detail,
        tavily_fallback: tavily_configured,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    fn hit(title: &str, url: &str, tier: u8) -> SearchHit {
        SearchHit {
            title: title.to_string(),
            url: url.to_string(),
            host: host_of(url),
            snippet: None,
            published: None,
            tier,
        }
    }

    #[test]
    fn interpret_searxng_names_the_json_misconfiguration() {
        let err = interpret_searxng(403, "").unwrap_err().to_string();
        assert!(err.contains("JSON output"), "{err}");
        let err = interpret_searxng(429, "").unwrap_err().to_string();
        assert!(err.contains("bot limiter"), "{err}");
        assert!(interpret_searxng(200, "{}").unwrap().results.is_empty());
        assert!(interpret_searxng(200, "not json").is_err());
    }

    #[test]
    fn searxng_round_trips_a_query() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[
                {"title":"Widget Co beats","url":"https://www.reuters.com/business/widget","content":"beat","publishedDate":"2026-08-20"},
                {"title":"","url":"https://x.example/no-title"},
                {"title":"no url","url":""}
            ]}"#,
        }]);
        let client = SearxngClient::new(&server.base_url).unwrap();
        let hits = client.search_raw("widget co earnings").unwrap();
        assert_eq!(server.attempts(), 1);
        assert_eq!(hits.len(), 1, "incomplete results are dropped");
        assert_eq!(hits[0].host, "reuters.com");
        assert_eq!(hits[0].tier, 2);
        assert_eq!(hits[0].published.as_deref(), Some("2026-08-20"));
        // The mock records the path sans query string; the JSON-format query
        // itself is pinned by the 403 arm above (a non-JSON instance 403s).
        assert_eq!(server.request_paths(), ["/search"]);
    }

    #[test]
    fn filter_drops_denied_hosts_and_collapses_syndication() {
        let hits = vec![
            hit("Widget Co beats on revenue", "https://reuters.com/a", 2),
            // Same story syndicated by a lower-tier outlet: collapses into the wire copy.
            hit("Widget Co Beats on Revenue!", "https://randomblog.example/b", 4),
            // A denied host is dropped outright.
            hit("Widget Co forecast 2030", "https://stockinvest.us/widget", 9),
            // A distinct story survives.
            hit("Widget Co CFO resigns", "https://apnews.com/c", 2),
            // An exact duplicate URL is dropped.
            hit("Widget Co beats on revenue", "https://reuters.com/a", 2),
        ];
        let kept = filter_and_collapse(hits);
        let urls: Vec<&str> = kept.iter().map(|h| h.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["https://reuters.com/a", "https://apnews.com/c"],
            "{kept:?}"
        );
    }

    #[test]
    fn syndication_keeps_the_higher_tier_origin_whatever_the_order() {
        let hits = vec![
            hit("Widget Co beats on revenue", "https://randomblog.example/b", 4),
            hit("Widget Co beats on revenue", "https://reuters.com/a", 2),
        ];
        let kept = filter_and_collapse(hits);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].host, "reuters.com", "higher tier wins from behind");
    }

    struct ScriptedBackend(Vec<crate::news::RawHeadline>);
    impl crate::research_executor::SearchBackend for ScriptedBackend {
        fn search(&self, _query: &str) -> anyhow::Result<Vec<crate::news::RawHeadline>> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn fallback_engages_when_searxng_is_absent_or_failing() {
        let tavily = ScriptedBackend(vec![crate::news::RawHeadline {
            title: "Widget Co beats".into(),
            url: "https://reuters.com/a".into(),
            source: "reuters.com".into(),
            published: None,
            snippet: None,
        }]);
        // No SearXNG configured at all -> straight to Tavily, route recorded.
        let search = FallbackSearch::new(None, Some(Box::new(tavily)));
        let (hits, route) = search.search_routed("widget co").unwrap();
        assert_eq!(route, SearchRoute::TavilyFallback);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn fallback_engages_when_every_searxng_hit_is_filtered() {
        // SearXNG responds, but every hit dies at the rank-time filter (a
        // denied host) — as unservable as a raw empty set, so Tavily engages.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[{"title":"Buy signal!","url":"https://stockinvest.us/x","content":""}]}"#,
        }]);
        let tavily = ScriptedBackend(vec![crate::news::RawHeadline {
            title: "Widget Co beats".into(),
            url: "https://reuters.com/a".into(),
            source: "reuters.com".into(),
            published: None,
            snippet: None,
        }]);
        let search = FallbackSearch::new(
            Some(SearxngClient::new(&server.base_url).unwrap()),
            Some(Box::new(tavily)),
        );
        let (hits, route) = search.search_routed("widget co").unwrap();
        assert_eq!(route, SearchRoute::TavilyFallback);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn preflight_reads_ok_unreachable_and_not_configured() {
        // Not configured: no probe at all.
        let p = preflight(None, true);
        assert_eq!(p.status, "not_configured");
        assert!(p.degraded);
        assert!(p.tavily_fallback);
        let p = preflight(Some("   "), false);
        assert_eq!(p.status, "not_configured");
        assert!(!p.tavily_fallback);

        // A serving instance — one that returns results — reads ok.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[{"title":"Markets today","url":"https://reuters.com/a","content":""}]}"#,
        }]);
        let p = preflight(Some(&server.base_url), false);
        assert_eq!(p.status, "ok");
        assert!(!p.degraded);

        // An instance that responds but returns NOTHING reads degraded — the
        // search path treats zero results as fallback-triggering, so a probe
        // that called this healthy would silently consent to metered Tavily
        // spend on every actual search.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[]}"#,
        }]);
        let p = preflight(Some(&server.base_url), false);
        assert_eq!(p.status, "unreachable");
        assert!(p.degraded);
        assert!(p.detail.unwrap().contains("no usable results"));

        // Same for a result set the rank-time filter empties (denied hosts
        // only) — the probe judges through the SAME filter the run uses, so
        // preflight and fallback can never disagree on this result class.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[{"title":"Buy signal!","url":"https://stockinvest.us/x","content":""}]}"#,
        }]);
        let p = preflight(Some(&server.base_url), false);
        assert_eq!(p.status, "unreachable");
        assert!(p.degraded);

        // A JSON-disabled instance reads unreachable with the diagnosis.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 403,
            headers: vec![],
            body: "",
        }]);
        let p = preflight(Some(&server.base_url), true);
        assert_eq!(p.status, "unreachable");
        assert!(p.degraded);
        assert!(p.detail.unwrap().contains("JSON output"));
    }

    #[test]
    fn no_backend_at_all_is_an_error_the_loop_fail_softs() {
        let search = FallbackSearch::new(None, None);
        let err = search.search_routed("widget co").unwrap_err().to_string();
        assert!(err.contains("no Tavily fallback"), "{err}");
    }
}
