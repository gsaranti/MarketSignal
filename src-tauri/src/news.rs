//! The news-ingestion source contract: a structured raw-headline boundary for
//! Step 7 (`docs/weekly-report-workflow.md §Step 7`).
//!
//! Mirrors the `data_sources` spine — the application layer owns all I/O, each
//! news source is a trait the orchestrator drives, and a deterministic stub
//! stands in for the live providers in offline tests. Three real adapters
//! implement this trait — `tavily` (AI-oriented market / research headlines),
//! `gdelt` (geopolitical and large-scale news-trend coverage), and `fmp_news`
//! (FMP Articles, ticker-tagged company-level commentary) — and the
//! `CompositeNewsSource` below nests to run them all and concatenate their
//! headlines.
//!
//! Step 7 gathers a broad set of raw headlines (this module), then a fixed
//! low-cost model dedupes / scores / clusters them down to the important stories
//! (the `headline_filter` stage). This module owns the gathering half plus the
//! deterministic exact-duplicate pre-pass (`dedupe_headlines`) that trims obvious
//! repeats before the model ever sees them; `pipeline::assemble_research_packet`
//! runs both halves at the head of the research phase, feeding research routing
//! (Step 8).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// The market-news topics the gathering pass queries each provider for, derived
/// from the Step-3 news categories (`docs/weekly-report-workflow.md §Step 3` —
/// politics, geopolitics, China/trade, energy, earnings, AI/semiconductors, and
/// major economic developments). Plain free-text queries, accepted by both
/// Tavily's search API and GDELT's document query.
pub const NEWS_TOPICS: &[&str] = &[
    "US stock market policy and politics",
    "geopolitics affecting global markets",
    "China trade and tariffs",
    "energy and oil prices",
    "corporate earnings results",
    "AI and semiconductors",
    "US economy inflation and the Federal Reserve",
];

/// One raw headline gathered from a news source, before any filtering.
/// Provider-agnostic: Tavily, GDELT, and FMP Articles all map their results into
/// this shape, and the headline-filter stage will consume a flat list of them.
/// `published` and `snippet` are best-effort — a provider that omits them leaves
/// them `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawHeadline {
    pub title: String,
    pub url: String,
    /// The publisher / domain the headline came from (e.g. "reuters.com").
    pub source: String,
    pub published: Option<String>,
    pub snippet: Option<String>,
}

/// The news-source stage. One method: gather a broad set of raw headlines. Sync,
/// like the `MarketDataSource` trait — the blocking HTTP call inside each real
/// adapter is offloaded via `spawn_blocking` at the Tauri command seam.
pub trait NewsSource {
    fn gather(&self) -> anyhow::Result<Vec<RawHeadline>>;
}

/// Deterministic offline stand-in for the real news adapters. Returns a small,
/// fixed set of headlines so the gather path and its tests run without live keys.
#[derive(Debug, Default)]
pub struct StubNewsSource;

impl NewsSource for StubNewsSource {
    fn gather(&self) -> anyhow::Result<Vec<RawHeadline>> {
        Ok(vec![
            RawHeadline {
                title: "Fed holds rates steady as inflation cools".into(),
                url: "https://example.com/fed-holds".into(),
                source: "example.com".into(),
                published: Some("2026-06-05".into()),
                snippet: Some("The Federal Reserve left its target range unchanged.".into()),
            },
            RawHeadline {
                title: "AI chipmakers extend rally on datacenter demand".into(),
                url: "https://example.com/ai-chips".into(),
                source: "example.com".into(),
                published: Some("2026-06-04".into()),
                snippet: None,
            },
        ])
    }
}

/// Compose two `NewsSource`s into one gather: run the `primary`, then the
/// `secondary`, and concatenate their headlines (primary first). Mirrors
/// `data_sources::CompositeMarketDataSource`. Either child's error propagates;
/// per-provider degradation (GDELT's best-effort soft-skip) lives inside the
/// adapter, so the composite stays a plain concatenation.
pub struct CompositeNewsSource<P, S> {
    pub primary: P,
    pub secondary: S,
}

impl<P, S> CompositeNewsSource<P, S> {
    pub fn new(primary: P, secondary: S) -> Self {
        Self { primary, secondary }
    }
}

impl<P: NewsSource, S: NewsSource> NewsSource for CompositeNewsSource<P, S> {
    fn gather(&self) -> anyhow::Result<Vec<RawHeadline>> {
        let mut headlines = self.primary.gather()?;
        headlines.extend(self.secondary.gather()?);
        Ok(headlines)
    }
}

/// Collapse obvious duplicate headlines before the filtering model sees them: a
/// deterministic exact-duplicate pre-pass keyed first on a normalized URL, then
/// on a normalized title. Keeps the first occurrence and preserves order, so the
/// earliest provider's framing of a story survives. This trims the raw gather
/// (the same story syndicated across outlets, or returned by overlapping topic
/// queries) without displacing the model's later *semantic* dedup, relevance
/// scoring, and clustering (`docs/weekly-report-workflow.md §Step 7`).
pub fn dedupe_headlines(headlines: Vec<RawHeadline>) -> Vec<RawHeadline> {
    let mut seen_urls = HashSet::new();
    let mut seen_titles = HashSet::new();
    let mut out = Vec::with_capacity(headlines.len());
    for h in headlines {
        // An empty key is no key at all — an empty URL falls through to the title
        // check rather than collapsing every URL-less headline into one.
        let url_key = normalize_url(&h.url);
        let title_key = normalize_title(&h.title);
        // Check both keys before committing either: a headline rejected by one key
        // must not reserve the other and suppress a later, genuinely-unique headline.
        if !url_key.is_empty() && seen_urls.contains(&url_key) {
            continue;
        }
        if !title_key.is_empty() && seen_titles.contains(&title_key) {
            continue;
        }
        if !url_key.is_empty() {
            seen_urls.insert(url_key);
        }
        if !title_key.is_empty() {
            seen_titles.insert(title_key);
        }
        out.push(h);
    }
    out
}

/// Normalize a URL for duplicate detection: lowercased, scheme and a leading
/// `www.` dropped, trailing slash trimmed. Coarse on purpose — it catches the
/// http/https and www variants of the same link without trying to canonicalize
/// query strings.
fn normalize_url(url: &str) -> String {
    let mut u = url.trim().to_ascii_lowercase();
    for scheme in ["https://", "http://"] {
        if let Some(rest) = u.strip_prefix(scheme) {
            u = rest.to_string();
            break;
        }
    }
    if let Some(rest) = u.strip_prefix("www.") {
        u = rest.to_string();
    }
    u.trim_end_matches('/').to_string()
}

/// Normalize a title for duplicate detection: lowercased with inner whitespace
/// collapsed to single spaces. Catches the same headline reprinted with trivial
/// spacing differences.
fn normalize_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Normalize a publisher host for the `source` field: lowercase, scheme and a
/// leading `www.` dropped, path discarded. Accepts either a full URL
/// (`https://www.reuters.com/markets/x`) or a bare domain (GDELT's `WWW.Reuters.com`),
/// both yielding `reuters.com`. Lowercasing happens before the `www.` strip so an
/// uppercase host normalizes the same way a lowercase one does. A value with no
/// parseable host yields an empty string, which the caller may leave as-is.
pub(crate) fn host_of(url: &str) -> String {
    let lowered = url.trim().to_ascii_lowercase();
    let after_scheme = lowered.split("://").nth(1).unwrap_or(lowered.as_str());
    let host = after_scheme.split('/').next().unwrap_or("");
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headline(title: &str, url: &str) -> RawHeadline {
        RawHeadline {
            title: title.into(),
            url: url.into(),
            source: host_of(url),
            published: None,
            snippet: None,
        }
    }

    /// A stub that always fails its gather, to check composite error propagation.
    struct FailingNewsSource;
    impl NewsSource for FailingNewsSource {
        fn gather(&self) -> anyhow::Result<Vec<RawHeadline>> {
            anyhow::bail!("news source down")
        }
    }

    #[test]
    fn dedupe_collapses_url_and_title_duplicates_preserving_order() {
        let raw = vec![
            headline("Fed holds rates", "https://a.com/fed"),
            // Same link, http + www + trailing slash variant -> URL duplicate.
            headline("Fed Holds Rates (live)", "http://www.a.com/fed/"),
            // Distinct link but the same headline text, trivially respaced -> title dup.
            headline("Fed   holds   rates", "https://b.com/mirror"),
            headline("Oil spikes on supply fears", "https://c.com/oil"),
        ];
        let out = dedupe_headlines(raw);
        assert_eq!(out.len(), 2, "two stories survive: {out:?}");
        // First occurrence is the one kept, and order is preserved.
        assert_eq!(out[0].title, "Fed holds rates");
        assert_eq!(out[1].title, "Oil spikes on supply fears");
    }

    #[test]
    fn dedupe_does_not_let_a_title_dropped_headline_reserve_its_url() {
        // The middle headline is dropped on its title (a dup of the first) — it must
        // NOT reserve its fresh URL, or the third headline (same URL, unique title)
        // would be wrongly suppressed.
        let raw = vec![
            headline("Fed holds rates", "https://a.com/fed"),
            headline("Fed   holds   rates", "https://shared.com/x"),
            headline("Markets rally into the close", "https://shared.com/x"),
        ];
        let out = dedupe_headlines(raw);
        assert_eq!(
            out.len(),
            2,
            "the unique-title headline must survive: {out:?}"
        );
        assert_eq!(out[0].title, "Fed holds rates");
        assert_eq!(out[1].title, "Markets rally into the close");
    }

    #[test]
    fn dedupe_keeps_url_less_headlines_distinct() {
        // Two headlines with no URL but different titles must both survive — an
        // empty URL is not a dedup key.
        let raw = vec![headline("Story one", ""), headline("Story two", "")];
        assert_eq!(dedupe_headlines(raw).len(), 2);
    }

    #[test]
    fn composite_concatenates_primary_then_secondary() {
        struct One;
        impl NewsSource for One {
            fn gather(&self) -> anyhow::Result<Vec<RawHeadline>> {
                Ok(vec![headline("primary", "https://p.com/1")])
            }
        }
        let composite = CompositeNewsSource::new(One, StubNewsSource);
        let out = composite.gather().unwrap();
        assert_eq!(out[0].title, "primary", "primary headline comes first");
        assert!(out.len() > 1, "secondary headlines follow");
    }

    #[test]
    fn composite_propagates_a_source_failure() {
        let composite = CompositeNewsSource::new(StubNewsSource, FailingNewsSource);
        assert!(composite.gather().is_err());
    }

    #[test]
    fn host_of_extracts_the_domain() {
        assert_eq!(host_of("https://www.reuters.com/markets/x"), "reuters.com");
        assert_eq!(host_of("http://bloomberg.com/news"), "bloomberg.com");
        assert_eq!(host_of("reuters.com/x"), "reuters.com");
        assert_eq!(host_of(""), "");
        // Accepts a bare domain (GDELT's shape) and normalizes an uppercase www.
        assert_eq!(host_of("WWW.Reuters.com"), "reuters.com");
        assert_eq!(host_of("HTTPS://WWW.Bloomberg.com/n"), "bloomberg.com");
    }

    #[test]
    fn raw_headline_round_trips_through_serde() {
        let h = headline("S&P 500 closes higher", "https://x.com/spx");
        let json = serde_json::to_string(&h).unwrap();
        let back: RawHeadline = serde_json::from_str(&json).unwrap();
        assert_eq!(h, back);
    }

    #[test]
    #[ignore = "hits live Tavily + GDELT; set TAVILY_API_KEY (GDELT is keyless)"]
    fn news_ingestion_smoke() {
        let tavily = crate::tavily::TavilyNewsSource::from_env().expect("TAVILY_API_KEY set");
        let gdelt = crate::gdelt::GdeltNewsSource::new().expect("gdelt client");
        let source = CompositeNewsSource::new(tavily, gdelt);
        let raw = source.gather().expect("gather headlines");
        assert!(
            !raw.is_empty(),
            "expected some headlines from the live gather"
        );
        let deduped = dedupe_headlines(raw.clone());
        assert!(deduped.len() <= raw.len(), "dedup never grows the set");
        assert!(!deduped.is_empty(), "dedup kept at least one headline");
        for h in &deduped {
            assert!(!h.title.trim().is_empty(), "kept headline has a title");
            assert!(!h.url.trim().is_empty(), "kept headline has a url");
        }
        eprintln!(
            "news ingestion smoke: {} raw -> {} deduped headlines",
            raw.len(),
            deduped.len()
        );
    }
}
