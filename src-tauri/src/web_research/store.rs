//! The web tool's two shared stores (`docs/storage.md §Local Analysis Suite
//! Storage`): the **document cache** and the **source state**. Both are shared
//! infrastructure, deliberately not job-partitioned — a fetched document is a
//! property of the URL and extraction behavior a property of the domain, not
//! of a job's learnings — so they sit beside the price-bar cache, outside the
//! per-job vector partitions. House style mirrors `crate::storage`: free
//! functions over `&rusqlite::Connection`, schema provisioned from
//! `storage::init_schema`.
//!
//! The document cache is the cross-run per-fetch layer both jobs' failure
//! postures name: fetched, readability-extracted documents keyed by
//! **normalized URL**, each carrying its **original retrieval timestamp** —
//! the immutable evidence vintage, never rewritten on reuse — and served only
//! within the shared ~4-week freshness window (older entries age out).
//! Portfolio's higher-level distilled-findings layer is a separate,
//! job-partitioned store over these same fetches.
//!
//! The source state is the learned layer the fetch telemetry accumulates:
//! per-domain full-vs-thin recovery counts, the resolved `extractionProfile`,
//! and the derived **render-first flag** — persisted now, consumed by the
//! deferred rendered-retrieval tier when that slice lands.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::fetch::FetchedPage;
use super::registry::ExtractionProfile;

/// The suite's shared research-freshness window (`docs/portfolio-analysis.md
/// §Starting parameters` — the ~4-week seed/credit window; the document cache
/// ages out on the same bound).
pub const RESEARCH_FRESHNESS_DAYS: i64 = 28;

/// Telemetry floor before the learned profile overrides the seeded heuristic:
/// below this many observed fetches a domain keeps its registry default.
/// Drafted, calibratable.
const PROFILE_MIN_SAMPLES: i64 = 3;

/// Thin-recovery share at or above which a domain reads `js_required` and the
/// render-first flag sets (skip the wasted GET once the render tier exists).
/// Drafted, calibratable.
const RENDER_FIRST_THIN_RATIO: f64 = 0.8;

/// Create the web-research tables if absent. Idempotent; called from
/// `storage::init_schema`. Both tables are exported by data portability
/// (format v4): cached documents let an imported corpus's research reuse work
/// offline, and the source state is learned analytical behavior that does not
/// regenerate quickly. Import pre-checks mirror the two primary keys.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS web_documents (
            url                TEXT PRIMARY KEY,
            host               TEXT NOT NULL,
            retrieved_at       TEXT NOT NULL,
            title              TEXT NOT NULL,
            text               TEXT NOT NULL,
            extraction_quality REAL NOT NULL,
            thin_stub          INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS web_source_state (
            host         TEXT PRIMARY KEY,
            full_count   INTEGER NOT NULL,
            thin_count   INTEGER NOT NULL,
            profile      TEXT,
            render_first INTEGER NOT NULL,
            updated_at   TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

/// Normalize a URL for the cache key: scheme+host lowercased, the fragment
/// dropped, a trailing slash trimmed. The query string is kept — it addresses
/// distinct documents on article CMSes.
pub fn normalize_url(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut u) => {
            u.set_fragment(None);
            u.to_string().trim_end_matches('/').to_string()
        }
        Err(_) => url.trim().trim_end_matches('/').to_string(),
    }
}

/// Cache a fetched document under its normalized final URL. A live re-fetch
/// of the same URL replaces the row whole — content and vintage together, a
/// fresh retrieval; *reuse* never rewrites `retrieved_at` because reads never
/// write.
pub fn put_document(conn: &Connection, page: &FetchedPage) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO web_documents
             (url, host, retrieved_at, title, text, extraction_quality, thin_stub)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            normalize_url(&page.final_url),
            page.host,
            page.retrieved_at,
            page.title,
            page.text,
            page.extraction_quality,
            page.thin_stub as i64,
        ],
    )?;
    Ok(())
}

/// A cached document, served only while its own retrieval vintage is inside
/// the shared freshness window — an expired entry reads as absent.
pub fn get_fresh_document(
    conn: &Connection,
    url: &str,
    now: DateTime<Utc>,
) -> Result<Option<FetchedPage>> {
    let row = conn
        .query_row(
            "SELECT url, host, retrieved_at, title, text, extraction_quality, thin_stub
             FROM web_documents WHERE url = ?1",
            params![normalize_url(url)],
            |r| {
                Ok(FetchedPage {
                    final_url: r.get::<_, String>(0)?,
                    host: r.get(1)?,
                    retrieved_at: r.get(2)?,
                    title: r.get(3)?,
                    text: r.get(4)?,
                    extraction_quality: r.get(5)?,
                    thin_stub: r.get::<_, i64>(6)? != 0,
                })
            },
        )
        .optional()?;
    Ok(row.filter(|page| {
        DateTime::parse_from_rfc3339(&page.retrieved_at)
            .map(|t| now.signed_duration_since(t.with_timezone(&Utc)).num_days()
                < RESEARCH_FRESHNESS_DAYS)
            .unwrap_or(false)
    }))
}

/// Age out cache entries past the freshness window. Returns the pruned count.
pub fn prune_expired_documents(conn: &Connection, now: DateTime<Utc>) -> Result<usize> {
    let cutoff = (now - chrono::Duration::days(RESEARCH_FRESHNESS_DAYS)).to_rfc3339();
    // RFC 3339 UTC strings order lexicographically within the same offset.
    let n = conn.execute(
        "DELETE FROM web_documents WHERE retrieved_at < ?1",
        params![cutoff],
    )?;
    Ok(n)
}

/// One domain's learned extraction state.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceState {
    pub host: String,
    pub full_count: i64,
    pub thin_count: i64,
    /// The telemetry-resolved profile once samples clear the floor; `None`
    /// while the registry/heuristic default still governs.
    pub profile: Option<ExtractionProfile>,
    /// A domain repeatedly thin to a plain GET: the deferred render tier will
    /// skip straight to the webview render. Persisted, unconsumed this slice.
    pub render_first: bool,
}

/// Record one fetch's extraction outcome for a domain and re-derive its
/// learned profile + render-first flag (`docs/web-research.md §Extraction
/// telemetry`).
pub fn record_fetch_outcome(
    conn: &Connection,
    host: &str,
    thin_stub: bool,
    now: DateTime<Utc>,
) -> Result<SourceState> {
    let host = super::registry::normalize_host(host);
    let existing = source_state(conn, &host)?;
    let (mut full, mut thin) = existing
        .as_ref()
        .map(|s| (s.full_count, s.thin_count))
        .unwrap_or((0, 0));
    if thin_stub {
        thin += 1;
    } else {
        full += 1;
    }
    let total = full + thin;
    let thin_ratio = thin as f64 / total as f64;
    let (profile, render_first) = if total < PROFILE_MIN_SAMPLES {
        (None, false)
    } else if thin_ratio >= RENDER_FIRST_THIN_RATIO {
        (Some(ExtractionProfile::JsRequired), true)
    } else {
        (Some(ExtractionProfile::Html), false)
    };
    let profile_str = profile.map(|p| match p {
        ExtractionProfile::ApiOrHtml => "api_or_html",
        ExtractionProfile::Html => "html",
        ExtractionProfile::JsRequired => "js_required",
    });
    conn.execute(
        "INSERT OR REPLACE INTO web_source_state
             (host, full_count, thin_count, profile, render_first, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            host,
            full,
            thin,
            profile_str,
            render_first as i64,
            now.to_rfc3339()
        ],
    )?;
    Ok(SourceState {
        host,
        full_count: full,
        thin_count: thin,
        profile,
        render_first,
    })
}

/// A domain's learned state, when any fetch has been recorded.
pub fn source_state(conn: &Connection, host: &str) -> Result<Option<SourceState>> {
    let host = super::registry::normalize_host(host);
    conn.query_row(
        "SELECT host, full_count, thin_count, profile, render_first
         FROM web_source_state WHERE host = ?1",
        params![host],
        |r| {
            Ok(SourceState {
                host: r.get(0)?,
                full_count: r.get(1)?,
                thin_count: r.get(2)?,
                profile: match r.get::<_, Option<String>>(3)?.as_deref() {
                    Some("api_or_html") => Some(ExtractionProfile::ApiOrHtml),
                    Some("html") => Some(ExtractionProfile::Html),
                    Some("js_required") => Some(ExtractionProfile::JsRequired),
                    _ => None,
                },
                render_first: r.get::<_, i64>(4)? != 0,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn page(url: &str, retrieved_at: &str) -> FetchedPage {
        FetchedPage {
            final_url: url.to_string(),
            host: "reuters.com".to_string(),
            title: "t".to_string(),
            text: "body".to_string(),
            extraction_quality: 0.9,
            thin_stub: false,
            retrieved_at: retrieved_at.to_string(),
        }
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn normalize_url_drops_fragment_and_trailing_slash_keeps_query() {
        assert_eq!(
            normalize_url("HTTPS://Reuters.com/a/?id=2#section"),
            "https://reuters.com/a/?id=2"
        );
        assert_eq!(
            normalize_url("https://reuters.com/a/"),
            "https://reuters.com/a"
        );
    }

    #[test]
    fn cache_serves_only_inside_the_freshness_window() {
        let conn = mem_conn();
        put_document(&conn, &page("https://reuters.com/a", "2026-08-01T12:00:00+00:00")).unwrap();

        // Fresh inside the window (27 days later).
        let now = at("2026-08-28T12:00:00+00:00");
        assert!(get_fresh_document(&conn, "https://reuters.com/a", now)
            .unwrap()
            .is_some());
        // The fragment-insensitive key serves the same row.
        assert!(
            get_fresh_document(&conn, "https://reuters.com/a#top", now)
                .unwrap()
                .is_some()
        );
        // Past the window it reads absent…
        let later = at("2026-08-29T13:00:00+00:00");
        assert!(get_fresh_document(&conn, "https://reuters.com/a", later)
            .unwrap()
            .is_none());
        // …and prunes.
        assert_eq!(prune_expired_documents(&conn, later).unwrap(), 1);
    }

    #[test]
    fn a_refetch_replaces_content_and_vintage_together() {
        let conn = mem_conn();
        put_document(&conn, &page("https://reuters.com/a", "2026-08-01T12:00:00+00:00")).unwrap();
        let mut fresh = page("https://reuters.com/a", "2026-08-20T12:00:00+00:00");
        fresh.text = "updated body".to_string();
        put_document(&conn, &fresh).unwrap();
        let now = at("2026-08-23T00:00:00+00:00");
        let got = get_fresh_document(&conn, "https://reuters.com/a", now)
            .unwrap()
            .unwrap();
        assert_eq!(got.text, "updated body");
        assert_eq!(got.retrieved_at, "2026-08-20T12:00:00+00:00");
    }

    #[test]
    fn an_unparseable_vintage_reads_absent_never_fresh() {
        let conn = mem_conn();
        put_document(&conn, &page("https://reuters.com/a", "not-a-date")).unwrap();
        let now = at("2026-08-23T00:00:00+00:00");
        assert!(get_fresh_document(&conn, "https://reuters.com/a", now)
            .unwrap()
            .is_none());
    }

    #[test]
    fn telemetry_learns_a_js_required_render_first_domain() {
        let conn = mem_conn();
        let now = at("2026-08-23T00:00:00+00:00");
        // Below the sample floor: no learned profile yet.
        let s = record_fetch_outcome(&conn, "www.Bloomberg.com", true, now).unwrap();
        assert_eq!(s.host, "bloomberg.com");
        assert_eq!(s.profile, None);
        assert!(!s.render_first);
        record_fetch_outcome(&conn, "bloomberg.com", true, now).unwrap();
        // Third observation, all thin: profile resolves js_required + render-first.
        let s = record_fetch_outcome(&conn, "bloomberg.com", true, now).unwrap();
        assert_eq!(s.profile, Some(ExtractionProfile::JsRequired));
        assert!(s.render_first);
        // A healthy full-text run re-derives back toward html.
        for _ in 0..7 {
            record_fetch_outcome(&conn, "bloomberg.com", false, now).unwrap();
        }
        let s = source_state(&conn, "bloomberg.com").unwrap().unwrap();
        assert_eq!(s.profile, Some(ExtractionProfile::Html));
        assert!(!s.render_first);
        assert_eq!(s.full_count, 7);
        assert_eq!(s.thin_count, 3);
    }
}
