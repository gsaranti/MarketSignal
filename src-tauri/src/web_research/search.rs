//! The web tool's search backend: self-hosted SearXNG, SearXNG-only — Tavily is
//! reserved for the report job, so there is no fallback here
//! (`docs/web-research.md §Search backend: SearXNG`, §Tavily fallback).
//!
//! SearXNG is queried over its JSON API on the configured (loopback)
//! endpoint — the one deliberate loopback target in the web tool, an
//! app-configured service address rather than a model-chosen fetch, so it is
//! exempt from the fetch guard's loopback block. It sits **off the execution
//! gate**: unreachable means a degraded run behind a pre-run notice, never a
//! block. When SearXNG can't serve (unreachable, misconfigured — an HTTP 403
//! is the JSON-output-disabled signature — or empty), the search fails; there
//! is no Tavily fallback (SearXNG-only), so the run researches without that
//! evidence.
//!
//! Rank-time policy lives here too: denied hosts are dropped at the search
//! filter, and near-duplicate / same-origin hits collapse so syndication
//! can't inflate apparent corroboration (five outlets reprinting one wire
//! story are one source, not five).

use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// Minimum spacing between consecutive SearXNG queries. The upstream public
/// engines rate-limit / CAPTCHA on burst rate from a single egress IP, and the
/// blocks are upstream, not the local limiter — so client-side pacing is the
/// keyless lever (`docs/web-research.md §Search backend: SearXNG`). Calibratable
/// and generous against the thinking-dominated ~25 min/holding cost: only
/// back-to-back queries within a model turn actually wait, since a full 122B
/// turn already spaces queries far past this between turns.
const MIN_SEARCH_INTERVAL: Duration = Duration::from_secs(4);

/// Jitter ceiling added on top of the interval so a run's bursts don't
/// re-synchronize into a fresh burst; applied only after the first query.
const SEARCH_JITTER: Duration = Duration::from_millis(1_500);

/// Cap on the run-scoped query cache — distinct queries in a run are naturally
/// bounded (order-of dozens per holding), so this only guards a pathological
/// run from unbounded growth; past it, results still compute, just uncached.
const MAX_QUERY_CACHE: usize = 4_096;

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

/// Run-scoped pacer that spaces outbound SearXNG queries. One instance
/// throttles every holding's searches, because the burst that trips the
/// upstream engines originates from the one shared egress IP across the whole
/// run — so pacing is a property of the client, not of any single holding.
struct SearchPacer {
    /// Minimum spacing between consecutive queries.
    min_interval: Duration,
    /// Jitter ceiling added on top (0 disables).
    jitter: Duration,
    /// The instant the last query was released (its scheduled release, so a
    /// reservation paces from when the prior query actually went out, not from
    /// now). `None` before the first query.
    last: Mutex<Option<Instant>>,
}

impl SearchPacer {
    fn new(min_interval: Duration, jitter: Duration) -> Self {
        Self {
            min_interval,
            jitter,
            last: Mutex::new(None),
        }
    }

    /// Reserve the next release slot and return how long to wait before it.
    /// The caller sleeps for the returned duration — kept sleep-free so the
    /// pacing math is deterministically testable. Only a query still inside the
    /// interval since the last one waits: the first query, and any query already
    /// spaced past the interval (a full model turn elapsed), waits nothing.
    /// Jitter rides only on a query that is actually being paced.
    fn reserve(&self, now: Instant) -> Duration {
        let mut last = self.last.lock().unwrap();
        let wait = match *last {
            Some(prev) => {
                let base = self.min_interval.saturating_sub(now.saturating_duration_since(prev));
                // A query already spaced past the interval needs no jitter —
                // jitter desynchronizes bursts, and this one isn't one.
                if base.is_zero() {
                    Duration::ZERO
                } else {
                    base + self.next_jitter()
                }
            }
            None => Duration::ZERO,
        };
        *last = Some(now + wait);
        wait
    }

    /// A cheap, dependency-free jitter in `[0, self.jitter)`. Not cryptographic
    /// — desynchronization, not unpredictability, is the goal — so the subsec
    /// nanos of the wall clock are a fine entropy source.
    fn next_jitter(&self) -> Duration {
        if self.jitter.is_zero() {
            return Duration::ZERO;
        }
        let span = self.jitter.as_millis().max(1) as u64;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        Duration::from_millis(nanos % span)
    }
}

/// The local SearXNG instance over its JSON API.
pub struct SearxngClient {
    base_url: String,
    http: reqwest::blocking::Client,
    pacer: SearchPacer,
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
            pacer: SearchPacer::new(MIN_SEARCH_INTERVAL, SEARCH_JITTER),
        })
    }

    /// One JSON search. Errors carry the misconfiguration diagnosis where the
    /// status makes one legible.
    pub fn search_raw(&self, query: &str) -> Result<Vec<SearchHit>> {
        let url = format!("{}/search", self.base_url);
        // Pace the outbound query: the upstream engines block on burst rate
        // from one egress IP, so hold a minimum spacing between consecutive
        // SearXNG queries. Polled here at the request boundary; the first query
        // of a run never waits.
        let wait = self.pacer.reserve(Instant::now());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
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
    /// but serves nothing usable leaves every actual search empty, and the
    /// local suite is SearXNG-only (no Tavily fallback), so the probe must
    /// surface that rather than silently consenting to a blind run.
    pub fn health_check(&self) -> Result<()> {
        let hits = filter_and_collapse(self.search_raw("financial market news")?);
        if hits.is_empty() {
            bail!(
                "SearXNG responded but returned no usable results for the probe query — its \
                 engines may be failing or disabled, so a run would research blind (the local \
                 suite is SearXNG-only, with no Tavily fallback)"
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
// The search tool
// ---------------------------------------------------------------------------

/// The local suite's web search: self-hosted SearXNG with a run-scoped dedup
/// cache. **SearXNG-only** — there is no fallback, because Tavily is reserved
/// for the report job (`docs/web-research.md §Tavily fallback`). A SearXNG that
/// can't serve — unreachable, misconfigured, or returning nothing usable —
/// fails the search, and the research loop fail-softs to a thinner packet.
pub struct SearchTool {
    searxng: Option<SearxngClient>,
    /// Run-scoped cache of results by normalized query, so a repeated query
    /// across a topic's passes returns without re-hitting SearXNG (and its
    /// pacing).
    cache: Mutex<std::collections::HashMap<String, Vec<SearchHit>>>,
}

/// Normalize a query for the dedup cache: lowercased with whitespace runs
/// collapsed — nothing more. Punctuation is deliberately preserved, because
/// SearXNG treats some of it as operators (`!engine` / `!category`,
/// `:language`), so folding it away would let a distinct query collect an
/// earlier query's cached results. The cache is conservative on purpose: a
/// false hit would feed the model the wrong results, while a false miss costs
/// only one (paced) SearXNG query — so it dedups only queries that are textually
/// identical bar case and spacing.
fn normalize_query(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl SearchTool {
    pub fn new(searxng: Option<SearxngClient>) -> Self {
        Self {
            searxng,
            cache: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// One search, dedup-cached. The run-scoped cache short-circuits a repeated
    /// query before any network call; only successful results are cached (a
    /// failure stays retryable). "Returning nothing" is judged **after** the
    /// rank-time filter: a result set surviving only as denied or malformed
    /// entries is no more servable than a raw empty one, so it fails the same
    /// way — and with no fallback, that failure reaches the loop as a degraded
    /// search.
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>> {
        let key = normalize_query(query);
        if let Some(cached) = self.cache.lock().unwrap().get(&key).cloned() {
            return Ok(cached);
        }
        let result = self.search_uncached(query);
        if let Ok(hits) = &result {
            let mut cache = self.cache.lock().unwrap();
            if cache.len() < MAX_QUERY_CACHE {
                cache.insert(key, hits.clone());
            }
        }
        result
    }

    fn search_uncached(&self, query: &str) -> Result<Vec<SearchHit>> {
        match &self.searxng {
            Some(client) => {
                let filtered = filter_and_collapse(client.search_raw(query)?);
                if filtered.is_empty() {
                    bail!("SearXNG returned no usable results");
                }
                Ok(filtered)
            }
            None => bail!("no SearXNG endpoint configured"),
        }
    }
}

// ---------------------------------------------------------------------------
// Pre-run preflight
// ---------------------------------------------------------------------------

/// The pre-run web-research read (`docs/web-research.md §Tavily fallback` —
/// the pre-run notice; `docs/interface.md §Pre-run web-research notice`).
/// Never a gate: the frontend shows a confirm-and-proceed notice on a
/// degraded state, always flagged *not recommended* — the local suite is
/// SearXNG-only, so a degraded run researches blind — and the engine-only
/// paths never ask. There is no Tavily-fallback field because there is no
/// local-suite fallback: Tavily is reserved for the report job.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WebResearchPreflight {
    /// `ok` | `unreachable` | `not_configured`.
    pub status: String,
    /// The failure diagnosis when unreachable (e.g. the JSON-disabled 403).
    pub detail: Option<String>,
    /// True whenever SearXNG cannot serve — the notice trigger.
    pub degraded: bool,
}

/// Probe the configured SearXNG endpoint for the pre-run notice and the
/// Settings connection row. One bounded JSON query; no state change.
pub fn preflight(endpoint: Option<&str>) -> WebResearchPreflight {
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

    #[test]
    fn searxng_filtered_to_empty_is_a_degraded_error() {
        // SearXNG responds, but every hit dies at the rank-time filter (a denied
        // host) — as unservable as a raw empty set. SearXNG-only, so that
        // reaches the loop as a degraded (failed) search, never a fallback.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[{"title":"Buy signal!","url":"https://stockinvest.us/x","content":""}]}"#,
        }]);
        let search = SearchTool::new(Some(SearxngClient::new(&server.base_url).unwrap()));
        let err = search.search("widget co").unwrap_err().to_string();
        assert!(err.contains("no usable results"), "{err}");
    }

    #[test]
    fn preflight_reads_ok_unreachable_and_not_configured() {
        // Not configured: no probe at all.
        let p = preflight(None);
        assert_eq!(p.status, "not_configured");
        assert!(p.degraded);
        let p = preflight(Some("   "));
        assert_eq!(p.status, "not_configured");
        assert!(p.degraded);

        // A serving instance — one that returns results — reads ok.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[{"title":"Markets today","url":"https://reuters.com/a","content":""}]}"#,
        }]);
        let p = preflight(Some(&server.base_url));
        assert_eq!(p.status, "ok");
        assert!(!p.degraded);

        // An instance that responds but returns NOTHING reads degraded — the
        // search path treats zero usable results the same as an empty one, so a
        // probe that called this healthy would silently consent to a blind run.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[]}"#,
        }]);
        let p = preflight(Some(&server.base_url));
        assert_eq!(p.status, "unreachable");
        assert!(p.degraded);
        assert!(p.detail.unwrap().contains("no usable results"));

        // Same for a result set the rank-time filter empties (denied hosts
        // only) — the probe judges through the SAME filter the run uses, so
        // preflight and the search path can never disagree on this result class.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[{"title":"Buy signal!","url":"https://stockinvest.us/x","content":""}]}"#,
        }]);
        let p = preflight(Some(&server.base_url));
        assert_eq!(p.status, "unreachable");
        assert!(p.degraded);

        // A JSON-disabled instance reads unreachable with the diagnosis.
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 403,
            headers: vec![],
            body: "",
        }]);
        let p = preflight(Some(&server.base_url));
        assert_eq!(p.status, "unreachable");
        assert!(p.degraded);
        assert!(p.detail.unwrap().contains("JSON output"));
    }

    #[test]
    fn no_searxng_configured_is_an_error_the_loop_fail_softs() {
        let search = SearchTool::new(None);
        let err = search.search("widget co").unwrap_err().to_string();
        assert!(err.contains("no SearXNG endpoint configured"), "{err}");
    }

    #[test]
    fn pacer_spaces_consecutive_queries() {
        let pacer = SearchPacer::new(Duration::from_secs(4), Duration::ZERO);
        let base = Instant::now();
        // The first query of a run releases immediately.
        assert_eq!(pacer.reserve(base), Duration::ZERO);
        // A second query issued back-to-back waits the full interval.
        assert_eq!(pacer.reserve(base), Duration::from_secs(4));
        // Well past the interval since the reserved release: no wait.
        assert_eq!(pacer.reserve(base + Duration::from_secs(20)), Duration::ZERO);
    }

    #[test]
    fn pacer_jitter_rides_on_top_of_the_interval_after_the_first_query() {
        let pacer = SearchPacer::new(Duration::from_secs(4), Duration::from_millis(1_500));
        let base = Instant::now();
        // First query: immediate and jitter-free.
        assert_eq!(pacer.reserve(base), Duration::ZERO);
        // Second: the interval plus a jitter strictly under the ceiling.
        let wait = pacer.reserve(base);
        assert!(
            wait >= Duration::from_secs(4) && wait < Duration::from_millis(5_500),
            "jitter out of band: {wait:?}"
        );
    }

    #[test]
    fn query_cache_dedups_repeat_and_case_varied_searches() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"results":[{"title":"Widget Co beats","url":"https://www.reuters.com/business/widget","content":"beat"}]}"#,
        }]);
        let search = SearchTool::new(Some(SearxngClient::new(&server.base_url).unwrap()));
        let first = search.search("Widget Co earnings").unwrap();
        assert_eq!(first.len(), 1);
        // A repeat — and a case/whitespace-varied repeat — both serve from the
        // run-scoped cache, so SearXNG is hit exactly once.
        let again = search.search("Widget Co earnings").unwrap();
        let varied = search.search("  widget   co   EARNINGS ").unwrap();
        assert_eq!(again, first);
        assert_eq!(varied, first);
        assert_eq!(server.attempts(), 1, "repeat queries must not re-hit SearXNG");
    }

    #[test]
    fn normalize_query_folds_case_and_whitespace_but_keeps_punctuation() {
        assert_eq!(normalize_query("  Widget   Co  Earnings "), "widget co earnings");
        assert_eq!(normalize_query("widget co earnings"), "widget co earnings");
        // Punctuation is preserved: SearXNG operators must stay distinct, so a
        // `!engine` / `:lang` query never collides with the plain one.
        assert_eq!(normalize_query("!images   AAPL"), "!images aapl");
        assert_ne!(normalize_query("!images AAPL"), normalize_query("images AAPL"));
    }
}
