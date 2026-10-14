//! The web tool's two shared stores (`docs/storage.md §Local Analysis Suite
//! Storage`): the document cache and the per-domain source state. Deliberately
//! not job-partitioned — a fetched document is a property of the URL and
//! extraction behavior a property of the domain. Free functions over
//! `&rusqlite::Connection`, schema provisioned from `storage::init_schema`.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::fetch::FetchedPage;
use super::registry::ExtractionProfile;

/// The suite's shared research-freshness window (`docs/portfolio-analysis.md
/// §Starting parameters`); the document cache ages out on the same bound.
pub const RESEARCH_FRESHNESS_DAYS: i64 = 28;

/// Served fetches observed before the learned profile overrides the registry
/// default. Drafted, calibratable.
const PROFILE_MIN_SAMPLES: i64 = 3;

/// Thin-recovery share at or above which a domain reads `js_required` and
/// render-first. Drafted, calibratable.
const RENDER_FIRST_THIN_RATIO: f64 = 0.8;

/// Idempotent; called from `storage::init_schema`. Both tables are exported by
/// data portability, and import pre-checks mirror the two primary keys.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS web_documents (
            url                TEXT PRIMARY KEY,
            final_url          TEXT NOT NULL,
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
        "CREATE INDEX IF NOT EXISTS idx_web_documents_final_url
         ON web_documents(final_url)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS web_source_state (
            host         TEXT PRIMARY KEY,
            full_count   INTEGER NOT NULL,
            thin_count   INTEGER NOT NULL,
            failed_count INTEGER NOT NULL,
            denied_count INTEGER NOT NULL,
            profile      TEXT,
            render_first INTEGER NOT NULL,
            updated_at   TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

/// The cache key: scheme+host lowercased, the fragment dropped, a trailing
/// slash trimmed. The query string is kept — it addresses distinct documents
/// on article CMSes.
pub fn normalize_url(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut u) => {
            u.set_fragment(None);
            u.to_string().trim_end_matches('/').to_string()
        }
        Err(_) => url.trim().trim_end_matches('/').to_string(),
    }
}

/// Keyed by the normalized requested URL, with the normalized final URL kept
/// separately so a redirecting address hits on re-read. A re-fetch replaces
/// the row whole; reuse never rewrites `retrieved_at`, the evidence vintage.
pub fn put_document(conn: &Connection, requested_url: &str, page: &FetchedPage) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO web_documents
             (url, final_url, host, retrieved_at, title, text, extraction_quality, thin_stub)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            normalize_url(requested_url),
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

fn cached_page(r: &rusqlite::Row<'_>) -> rusqlite::Result<FetchedPage> {
    Ok(FetchedPage {
        final_url: r.get::<_, String>(0)?,
        host: r.get(1)?,
        retrieved_at: r.get(2)?,
        title: r.get(3)?,
        text: r.get(4)?,
        extraction_quality: r.get(5)?,
        thin_stub: r.get::<_, i64>(6)? != 0,
    })
}

fn page_is_fresh(page: &FetchedPage, now: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(&page.retrieved_at)
        .map(|t| {
            now.signed_duration_since(t.with_timezone(&Utc)).num_days()
                < RESEARCH_FRESHNESS_DAYS
        })
        .unwrap_or(false)
}

/// Looked up by the requested-URL key, then by final URL so a direct link to a
/// previously redirected destination reuses the row. Expired or malformed
/// vintages read absent.
pub fn get_fresh_document(
    conn: &Connection,
    url: &str,
    now: DateTime<Utc>,
) -> Result<Option<FetchedPage>> {
    let key = normalize_url(url);
    let exact = conn
        .query_row(
            "SELECT final_url, host, retrieved_at, title, text,
                    extraction_quality, thin_stub
             FROM web_documents WHERE url = ?1",
            params![key],
            cached_page,
        )
        .optional()?;
    if exact.as_ref().is_some_and(|page| page_is_fresh(page, now)) {
        return Ok(exact);
    }

    let mut stmt = conn.prepare(
        "SELECT final_url, host, retrieved_at, title, text,
                extraction_quality, thin_stub
         FROM web_documents
         WHERE final_url = ?1 AND url <> ?1
         ORDER BY retrieved_at DESC, url ASC",
    )?;
    let mut rows = stmt.query(params![key])?;
    while let Some(row) = rows.next()? {
        let page = cached_page(row)?;
        if page_is_fresh(&page, now) {
            return Ok(Some(page));
        }
    }
    Ok(None)
}

/// Returns the pruned count.
pub fn prune_expired_documents(conn: &Connection, now: DateTime<Utc>) -> Result<usize> {
    let cutoff = (now - chrono::Duration::days(RESEARCH_FRESHNESS_DAYS)).to_rfc3339();
    // RFC 3339 UTC strings order lexicographically within the same offset.
    let n = conn.execute(
        "DELETE FROM web_documents WHERE retrieved_at < ?1",
        params![cutoff],
    )?;
    Ok(n)
}

/// One live fetch's outcome (`docs/web-research.md §Extraction telemetry`).
/// `Full` / `Thin` are served pages. `Denied` is an HTTP 401/403 answer and
/// `Failed` any other failed live attempt past the app's own guard; neither
/// moves the profile. A policy refusal is never recorded — it is the app's
/// guard, not the source's behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchOutcome {
    Full,
    Thin,
    Denied,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceState {
    pub host: String,
    pub full_count: i64,
    pub thin_count: i64,
    /// Every error past the app's own policy guard.
    pub failed_count: i64,
    /// The `failed_count` subset the source answered HTTP 401/403.
    pub denied_count: i64,
    /// `None` while the registry/heuristic default still governs.
    pub profile: Option<ExtractionProfile>,
    /// A domain repeatedly thin to a plain GET. Persisted; no consumer yet.
    pub render_first: bool,
}

/// The profile reads served pages only — a failed attempt has no extraction
/// to learn from.
pub fn record_fetch_outcome(
    conn: &Connection,
    host: &str,
    outcome: FetchOutcome,
    now: DateTime<Utc>,
) -> Result<SourceState> {
    let host = super::registry::normalize_host(host);
    let existing = source_state(conn, &host)?;
    let (mut full, mut thin, mut failed, mut denied) = existing
        .as_ref()
        .map(|s| (s.full_count, s.thin_count, s.failed_count, s.denied_count))
        .unwrap_or((0, 0, 0, 0));
    match outcome {
        FetchOutcome::Full => full += 1,
        FetchOutcome::Thin => thin += 1,
        FetchOutcome::Denied => {
            failed += 1;
            denied += 1;
        }
        FetchOutcome::Failed => failed += 1,
    }
    let served = full + thin;
    let thin_ratio = if served == 0 {
        0.0
    } else {
        thin as f64 / served as f64
    };
    let (profile, render_first) = if served < PROFILE_MIN_SAMPLES {
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
             (host, full_count, thin_count, failed_count, denied_count, profile,
              render_first, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            host,
            full,
            thin,
            failed,
            denied,
            profile_str,
            render_first as i64,
            now.to_rfc3339()
        ],
    )?;
    Ok(SourceState {
        host,
        full_count: full,
        thin_count: thin,
        failed_count: failed,
        denied_count: denied,
        profile,
        render_first,
    })
}

pub fn source_state(conn: &Connection, host: &str) -> Result<Option<SourceState>> {
    let host = super::registry::normalize_host(host);
    conn.query_row(
        "SELECT host, full_count, thin_count, failed_count, denied_count, profile,
                render_first
         FROM web_source_state WHERE host = ?1",
        params![host],
        |r| {
            Ok(SourceState {
                host: r.get(0)?,
                full_count: r.get(1)?,
                thin_count: r.get(2)?,
                failed_count: r.get(3)?,
                denied_count: r.get(4)?,
                profile: match r.get::<_, Option<String>>(5)?.as_deref() {
                    Some("api_or_html") => Some(ExtractionProfile::ApiOrHtml),
                    Some("html") => Some(ExtractionProfile::Html),
                    Some("js_required") => Some(ExtractionProfile::JsRequired),
                    _ => None,
                },
                render_first: r.get::<_, i64>(6)? != 0,
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
    fn redirecting_requested_urls_hit_without_losing_the_final_url() {
        let conn = mem_conn();
        let requested = "https://reuters.com/go?id=7#tracking";
        let final_url = "https://www.reuters.com/world/widget-final/";
        put_document(
            &conn,
            requested,
            &page(final_url, "2026-08-20T12:00:00+00:00"),
        )
        .unwrap();
        let now = at("2026-08-23T00:00:00+00:00");
        let stored_key: String = conn
            .query_row("SELECT url FROM web_documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored_key, "https://reuters.com/go?id=7");

        let by_requested = get_fresh_document(&conn, requested, now)
            .unwrap()
            .expect("the redirecting seed key hits");
        assert_eq!(
            by_requested.final_url,
            "https://www.reuters.com/world/widget-final"
        );
        let by_final = get_fresh_document(&conn, final_url, now)
            .unwrap()
            .expect("the direct final URL hits too");
        assert_eq!(by_final, by_requested);
    }

    #[test]
    fn cache_serves_only_inside_the_freshness_window() {
        let conn = mem_conn();
        put_document(
            &conn,
            "https://reuters.com/a",
            &page("https://reuters.com/a", "2026-08-01T12:00:00+00:00"),
        )
        .unwrap();

        // 27 days later: inside the window.
        let now = at("2026-08-28T12:00:00+00:00");
        assert!(get_fresh_document(&conn, "https://reuters.com/a", now)
            .unwrap()
            .is_some());
        assert!(
            get_fresh_document(&conn, "https://reuters.com/a#top", now)
                .unwrap()
                .is_some()
        );
        let later = at("2026-08-29T13:00:00+00:00");
        assert!(get_fresh_document(&conn, "https://reuters.com/a", later)
            .unwrap()
            .is_none());
        assert_eq!(prune_expired_documents(&conn, later).unwrap(), 1);
    }

    #[test]
    fn a_refetch_replaces_content_and_vintage_together() {
        let conn = mem_conn();
        put_document(
            &conn,
            "https://reuters.com/a",
            &page("https://reuters.com/a", "2026-08-01T12:00:00+00:00"),
        )
        .unwrap();
        let mut fresh = page("https://reuters.com/a", "2026-08-20T12:00:00+00:00");
        fresh.text = "updated body".to_string();
        put_document(&conn, "https://reuters.com/a", &fresh).unwrap();
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
        put_document(
            &conn,
            "https://reuters.com/a",
            &page("https://reuters.com/a", "not-a-date"),
        )
        .unwrap();
        let now = at("2026-08-23T00:00:00+00:00");
        assert!(get_fresh_document(&conn, "https://reuters.com/a", now)
            .unwrap()
            .is_none());
    }

    #[test]
    fn telemetry_learns_a_js_required_render_first_domain() {
        let conn = mem_conn();
        let now = at("2026-08-23T00:00:00+00:00");
        let s = record_fetch_outcome(&conn, "www.Bloomberg.com", FetchOutcome::Thin, now).unwrap();
        assert_eq!(s.host, "bloomberg.com");
        assert_eq!(s.profile, None);
        assert!(!s.render_first);
        record_fetch_outcome(&conn, "bloomberg.com", FetchOutcome::Thin, now).unwrap();
        // The third observation clears the sample floor.
        let s = record_fetch_outcome(&conn, "bloomberg.com", FetchOutcome::Thin, now).unwrap();
        assert_eq!(s.profile, Some(ExtractionProfile::JsRequired));
        assert!(s.render_first);
        for _ in 0..7 {
            record_fetch_outcome(&conn, "bloomberg.com", FetchOutcome::Full, now).unwrap();
        }
        let s = source_state(&conn, "bloomberg.com").unwrap().unwrap();
        assert_eq!(s.profile, Some(ExtractionProfile::Html));
        assert!(!s.render_first);
        assert_eq!(s.full_count, 7);
        assert_eq!(s.thin_count, 3);
    }

    #[test]
    fn failed_and_denied_attempts_count_per_domain_without_moving_the_profile() {
        let conn = mem_conn();
        let now = at("2026-09-14T00:00:00+00:00");
        let s = record_fetch_outcome(&conn, "www.Reuters.com", FetchOutcome::Denied, now).unwrap();
        assert_eq!(s.host, "reuters.com");
        assert_eq!((s.failed_count, s.denied_count), (1, 1));
        let s = record_fetch_outcome(&conn, "reuters.com", FetchOutcome::Failed, now).unwrap();
        assert_eq!((s.failed_count, s.denied_count), (2, 1));
        assert_eq!((s.full_count, s.thin_count), (0, 0));
        assert_eq!(s.profile, None, "no served page, no learned profile");
        assert!(!s.render_first);
        // The failures do not dilute the thin ratio either.
        for _ in 0..3 {
            record_fetch_outcome(&conn, "reuters.com", FetchOutcome::Thin, now).unwrap();
        }
        let s = source_state(&conn, "reuters.com").unwrap().unwrap();
        assert_eq!(s.profile, Some(ExtractionProfile::JsRequired));
        assert!(s.render_first);
        assert_eq!(
            (s.failed_count, s.denied_count),
            (2, 1),
            "the attempt counters persist beside the extraction pair"
        );
    }
}
