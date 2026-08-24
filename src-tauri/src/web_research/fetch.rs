//! The web tool's page fetch and readability extraction
//! (`docs/web-research.md §Fetch and extraction`, §Safety and provenance).
//!
//! Because the model chooses what to fetch, fetching is an untrusted
//! operation. The guard here enforces the SSRF rules: `http`/`https` only,
//! public hosts only (private, loopback, and link-local ranges are blocked —
//! this matters specifically because the app's own Ollama and SearXNG run on
//! loopback), redirects capped and re-validated against the same rules, and
//! responses bounded by size and content type (HTML/text only). Resolved
//! addresses are pinned into the client so the connection goes to the
//! addresses that were validated, not a second DNS answer.
//!
//! The plain GET carries a realistic, browser-like header set — cheap
//! prevention so the common fetch isn't needlessly flagged as a bot; it won't
//! fool TLS-fingerprint detectors, which is the deferred render tier's job,
//! not the GET's. Extraction strips navigation, ads, and boilerplate down to
//! the article body (`dom_smoothie`, a Readability.js-faithful pure-Rust
//! extractor), and every fetch feeds the per-domain extraction telemetry that
//! will gate the deferred rendered-retrieval tier. Pages that are paywalled or
//! render client-side return thin text — a fetch-layer limit, not an extractor
//! failure — and simply contribute less evidence rather than breaking the loop.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Url;

use super::registry::{self, SourcePolicy};

/// Per-fetch timeout. A research fetch is one page, not a bulk pull; anything
/// slower is treated as unreachable and degrades fail-soft.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Redirect ceiling — each hop is re-validated against the SSRF rules.
const REDIRECT_CAP: usize = 5;

/// Response-body byte bound. Article pages sit well under this; anything
/// larger is truncated at the bound rather than buffered unbounded.
const MAX_FETCH_BYTES: u64 = 2_000_000;

/// Below this many extracted characters a page reads as a thin paywall / JS
/// stub (Mozilla's internal readerable threshold is 500 chars). Drafted.
const THIN_CHAR_THRESHOLD: usize = 500;

/// Extracted-character count treated as a full article body for the 0–1
/// `extraction_quality` read (quality = extracted / this, clamped). Drafted.
const FULL_BODY_CHARS: f64 = 2_500.0;

/// The browser-like header set (`docs/web-research.md §Fetch and extraction`).
/// A coherent macOS Safari-class set: UA plus the Accept / Accept-Language /
/// Sec-Fetch-* headers a real navigation sends.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                          AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";
const ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
const ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

/// One fetched, extracted page — the shape the research loop's evidence
/// ledger and the document cache consume. `retrieved_at` is the original
/// retrieval instant (RFC 3339 UTC), the immutable evidence vintage.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FetchedPage {
    /// The URL the content actually came from (after any redirects).
    pub final_url: String,
    pub host: String,
    pub title: String,
    /// The readability-extracted article text.
    pub text: String,
    /// 0–1: extracted body vs a full article (see `FULL_BODY_CHARS`).
    pub extraction_quality: f64,
    /// The paywall / JS-stub flag: too little body recovered to treat the
    /// fetch as the page's content.
    pub thin_stub: bool,
    pub retrieved_at: String,
}

/// The injectable fetch seam: the research runner, tests, and demo mode each
/// supply their own. The live implementation is [`HttpPageFetcher`].
pub trait PageFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<FetchedPage>;
}

/// Why an address is rejected — used in errors so a blocked fetch is legible
/// in the tracker row rather than a bare "failed".
fn non_public_reason(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            if v4.is_loopback() {
                Some("loopback address")
            } else if v4.is_private() {
                Some("private address")
            } else if v4.is_link_local() {
                Some("link-local address")
            } else if v4.is_unspecified() || v4.is_broadcast() || v4.is_multicast() || o[0] == 0 {
                Some("non-routable address")
            } else if o[0] == 100 && (o[1] & 0xC0) == 64 {
                Some("carrier-grade NAT address")
            } else if o[0] == 192 && o[1] == 0 && o[2] == 0 {
                // 192.0.0.0/24 — IETF protocol assignments (IANA special-use).
                Some("special-use (protocol-assignment) address")
            } else if o[0] == 198 && (o[1] & 0xFE) == 18 {
                // 198.18.0.0/15 — device benchmarking.
                Some("benchmarking address")
            } else if (o[0] == 192 && o[1] == 0 && o[2] == 2)
                || (o[0] == 198 && o[1] == 51 && o[2] == 100)
                || (o[0] == 203 && o[1] == 0 && o[2] == 113)
            {
                // TEST-NET-1/2/3 — documentation ranges.
                Some("documentation address")
            } else if o[0] >= 240 {
                // 240.0.0.0/4 — reserved (broadcast handled above).
                Some("reserved address")
            } else {
                None
            }
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            if v6.is_loopback() {
                Some("loopback address")
            } else if v6.is_unspecified() || v6.is_multicast() {
                Some("non-routable address")
            } else if (s[0] & 0xfe00) == 0xfc00 {
                Some("unique-local address")
            } else if (s[0] & 0xffc0) == 0xfe80 {
                Some("link-local address")
            } else if (s[0] & 0xffc0) == 0xfec0 {
                // fec0::/10 — deprecated site-local space.
                Some("deprecated site-local address")
            } else if s[0] == 0x2001 && s[1] == 0x0db8 {
                // 2001:db8::/32 — documentation range.
                Some("documentation address")
            } else if s[0] == 0x64 && s[1] == 0xff9b {
                // NAT64 (64:ff9b::/96): the embedded IPv4 target must pass the
                // v4 rules — treat the prefix itself as non-public.
                Some("NAT64-mapped address")
            } else if let Some(v4) = v6.to_ipv4() {
                // Covers both ::ffff:a.b.c.d mapped and deprecated ::a.b.c.d
                // v4-compatible forms — either embeds a v4 target that must
                // pass the v4 rules.
                non_public_reason(IpAddr::V4(v4))
            } else {
                None
            }
        }
    }
}

/// Resolve a URL's host and validate every address against the public-host
/// rules, returning the validated socket addresses so the connection can be
/// pinned to exactly what was checked (no second DNS answer).
fn resolve_public(url: &Url, allow_loopback: bool) -> Result<Vec<SocketAddr>> {
    let host = url
        .host_str()
        .context("fetch URL carries no host")?
        .to_string();
    let port = url
        .port_or_known_default()
        .context("fetch URL has no usable port")?;
    let addrs: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .with_context(|| format!("resolving {host}"))?
        .collect();
    if addrs.is_empty() {
        bail!("{host} resolved to no addresses");
    }
    for addr in &addrs {
        if allow_loopback && addr.ip().is_loopback() {
            continue;
        }
        if let Some(reason) = non_public_reason(addr.ip()) {
            bail!("fetch blocked: {host} resolves to a {reason}");
        }
    }
    Ok(addrs)
}

/// Validate a URL's scheme and host per the SSRF rules. Returns the pinned
/// addresses on success.
fn validate_url(url: &Url, allow_loopback: bool) -> Result<Vec<SocketAddr>> {
    match url.scheme() {
        "http" | "https" => {}
        other => bail!("fetch blocked: scheme {other:?} is not allowed"),
    }
    resolve_public(url, allow_loopback)
}

/// The no-network URL-policy check a **cache read** must pass before a stored
/// document may serve (`docs/web-research.md §Safety and provenance`): scheme,
/// the deny list, and the literal-address rules. A cache hit makes no request,
/// so DNS resolution is deliberately skipped — this guards against imported or
/// legacy cache rows bypassing the *current* source policy, not against SSRF
/// (no connection is opened on a hit).
pub fn check_url_policy(url_str: &str) -> Result<()> {
    let url = Url::parse(url_str).with_context(|| format!("unparseable URL {url_str:?}"))?;
    match url.scheme() {
        "http" | "https" => {}
        other => bail!("blocked: scheme {other:?} is not allowed"),
    }
    let host = url.host_str().context("URL carries no host")?;
    if let SourcePolicy::Deny(reason) = registry::assess(host) {
        bail!("blocked: {host} is on the deny list ({reason})");
    }
    if let Ok(ip) = host.trim_matches(|c| c == '[' || c == ']').parse::<IpAddr>() {
        if let Some(reason) = non_public_reason(ip) {
            bail!("blocked: {host} is a {reason}");
        }
    }
    Ok(())
}

/// Extract the readable article from an HTML body. Returns the title, the
/// extracted text, and the readability gate's verdict.
fn extract_article(html: &str, url: &str) -> (String, String, bool) {
    let cfg = dom_smoothie::Config {
        text_mode: dom_smoothie::TextMode::Formatted,
        ..Default::default()
    };
    let mut readability = match dom_smoothie::Readability::new(html, Some(url), Some(cfg)) {
        Ok(r) => r,
        Err(_) => return (String::new(), String::new(), false),
    };
    let probably_readable = readability.is_probably_readable();
    match readability.parse() {
        Ok(article) => (
            article.title,
            article.text_content.trim().to_string(),
            probably_readable,
        ),
        Err(_) => (String::new(), String::new(), probably_readable),
    }
}

/// The extraction-quality read and the thin-stub flag from an extraction
/// (`docs/web-research.md §Extraction telemetry`): quality is extracted
/// characters against a full-body yardstick; thin is the readability gate
/// failing or the body landing under the stub threshold.
fn quality_of(extracted_chars: usize, probably_readable: bool) -> (f64, bool) {
    let quality = (extracted_chars as f64 / FULL_BODY_CHARS).clamp(0.0, 1.0);
    let thin = !probably_readable || extracted_chars < THIN_CHAR_THRESHOLD;
    (quality, thin)
}

/// The live SSRF-guarded fetcher.
pub struct HttpPageFetcher {
    /// Test-only escape hatch for the loopback block, so the wire path can be
    /// driven against a localhost mock. Compiled to `false` in production —
    /// the field is set only by the `cfg(test)` constructor below.
    allow_loopback: bool,
}

impl HttpPageFetcher {
    pub fn new() -> Self {
        Self {
            allow_loopback: false,
        }
    }

    /// Allow loopback targets so tests can drive the full fetch path against
    /// `test_http::MockHttp`. Test-only by construction.
    #[cfg(test)]
    pub fn allowing_loopback() -> Self {
        Self {
            allow_loopback: true,
        }
    }

    /// One validated GET, redirects handled manually so every hop re-passes
    /// the SSRF rules. Returns the final URL and the (bounded) body text.
    fn get_bounded(&self, start: &Url) -> Result<(Url, String)> {
        let mut url = start.clone();
        for _hop in 0..=REDIRECT_CAP {
            // The fetch-gate deny check (`docs/data-sources.md §Source
            // registry and evidence tiers`): a denied host is dropped even if
            // a redirect landed on it.
            let host = url.host_str().unwrap_or_default();
            if let SourcePolicy::Deny(reason) = registry::assess(host) {
                bail!("fetch blocked: {host} is on the deny list ({reason})");
            }
            let addrs = validate_url(&url, self.allow_loopback)?;
            let client = reqwest::blocking::Client::builder()
                .timeout(FETCH_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .resolve_to_addrs(host, &addrs)
                .user_agent(USER_AGENT)
                .build()
                .context("building the fetch client")?;
            let resp = client
                .get(url.clone())
                .header("Accept", ACCEPT)
                .header("Accept-Language", ACCEPT_LANGUAGE)
                .header("Sec-Fetch-Dest", "document")
                .header("Sec-Fetch-Mode", "navigate")
                .header("Sec-Fetch-Site", "none")
                .send()
                .with_context(|| format!("fetching {url}"))?;

            let status = resp.status();
            if status.is_redirection() {
                let location = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .context("redirect carried no Location header")?;
                url = url
                    .join(location)
                    .with_context(|| format!("joining redirect target {location:?}"))?;
                continue;
            }
            if !status.is_success() {
                bail!("fetch of {url} returned HTTP {}", status.as_u16());
            }
            // Content-type bound: HTML/text only — a research fetch reads
            // documents, never binaries.
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            let allowed = content_type.is_empty()
                || content_type.starts_with("text/")
                || content_type.starts_with("application/xhtml+xml")
                || content_type.starts_with("application/xml");
            if !allowed {
                bail!("fetch of {url} returned unsupported content type {content_type:?}");
            }
            // Size bound: read at most MAX_FETCH_BYTES whatever the declared
            // Content-Length says.
            use std::io::Read;
            let mut body = Vec::new();
            resp.take(MAX_FETCH_BYTES)
                .read_to_end(&mut body)
                .with_context(|| format!("reading the body of {url}"))?;
            let text = String::from_utf8_lossy(&body).into_owned();
            return Ok((url, text));
        }
        bail!("fetch of {start} exceeded the {REDIRECT_CAP}-redirect cap")
    }
}

impl Default for HttpPageFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl PageFetcher for HttpPageFetcher {
    fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let parsed = Url::parse(url).with_context(|| format!("parsing fetch URL {url:?}"))?;
        let (final_url, html) = self.get_bounded(&parsed)?;
        let (title, text, probably_readable) = extract_article(&html, final_url.as_str());
        let (extraction_quality, thin_stub) = quality_of(text.chars().count(), probably_readable);
        Ok(FetchedPage {
            host: registry::normalize_host(final_url.host_str().unwrap_or_default()),
            final_url: final_url.to_string(),
            title,
            text,
            extraction_quality,
            thin_stub,
            retrieved_at: chrono::Utc::now().to_rfc3339(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    #[test]
    fn schemes_other_than_http_are_blocked() {
        let fetcher = HttpPageFetcher::allowing_loopback();
        for url in ["ftp://example.com/x", "file:///etc/passwd", "gopher://x"] {
            let err = fetcher.fetch(url).unwrap_err().to_string();
            assert!(
                err.contains("not allowed") || err.contains("parsing"),
                "{url} -> {err}"
            );
        }
    }

    #[test]
    fn non_public_addresses_are_rejected_with_reasons() {
        use std::net::Ipv4Addr;
        let cases: [(IpAddr, &str); 16] = [
            (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), "loopback"),
            (IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)), "private"),
            (IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), "private"),
            (IpAddr::V4(Ipv4Addr::new(169, 254, 0, 5)), "link-local"),
            (IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)), "carrier-grade"),
            (IpAddr::V4(Ipv4Addr::new(0, 1, 2, 3)), "non-routable"),
            // The special-use ranges the first cut missed (Codex round 1):
            // protocol assignments, benchmarking, TEST-NET, reserved space.
            (IpAddr::V4(Ipv4Addr::new(192, 0, 0, 170)), "protocol-assignment"),
            (IpAddr::V4(Ipv4Addr::new(198, 18, 0, 1)), "benchmarking"),
            (IpAddr::V4(Ipv4Addr::new(198, 19, 255, 1)), "benchmarking"),
            (IpAddr::V4(Ipv4Addr::new(192, 0, 2, 7)), "documentation"),
            (IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)), "documentation"),
            (IpAddr::V4(Ipv4Addr::new(240, 0, 0, 1)), "reserved"),
            ("fd00::1".parse().unwrap(), "unique-local"),
            ("64:ff9b::7f00:1".parse().unwrap(), "NAT64"),
            ("fec0::1".parse().unwrap(), "site-local"),
            ("2001:db8::1".parse().unwrap(), "documentation"),
        ];
        for (ip, needle) in cases {
            let reason = non_public_reason(ip).unwrap_or_else(|| panic!("{ip} should be blocked"));
            assert!(reason.contains(needle), "{ip}: {reason}");
        }
        // A deprecated v4-compatible v6 address embedding a private target is
        // classified by its embedded v4.
        assert!(non_public_reason("::192.168.1.1".parse().unwrap()).is_some());
        // Public addresses pass.
        assert_eq!(non_public_reason("93.184.216.34".parse().unwrap()), None);
        assert_eq!(non_public_reason("2606:2800:220:1::1".parse().unwrap()), None);
    }

    #[test]
    fn cache_url_policy_precheck_blocks_denied_and_literal_addresses() {
        // The no-network policy check a cache read must pass: scheme, deny
        // list, literal-address classes — so an imported or legacy cache row
        // can't serve under a policy the current rules would block.
        assert!(check_url_policy("https://reuters.com/a").is_ok());
        let err = check_url_policy("ftp://reuters.com/a").unwrap_err().to_string();
        assert!(err.contains("scheme"), "{err}");
        let err = check_url_policy("https://stockinvest.us/x").unwrap_err().to_string();
        assert!(err.contains("deny list"), "{err}");
        let err = check_url_policy("http://192.168.1.10/admin").unwrap_err().to_string();
        assert!(err.contains("private"), "{err}");
        let err = check_url_policy("http://[fec0::1]/x").unwrap_err().to_string();
        assert!(err.contains("site-local"), "{err}");
    }

    #[test]
    fn production_guard_blocks_loopback_targets() {
        // The production fetcher (no test allowance) refuses a loopback URL —
        // the rule that protects the app's own Ollama / SearXNG.
        let fetcher = HttpPageFetcher::new();
        let err = fetcher
            .fetch("http://127.0.0.1:9/never")
            .unwrap_err()
            .to_string();
        assert!(err.contains("loopback"), "{err}");
    }

    const ARTICLE_HTML: &str = r#"<!doctype html><html><head><title>Widget Co beats</title></head>
    <body><nav><a href="/">Home</a><a href="/markets">Markets</a></nav>
    <article><h1>Widget Co beats on revenue</h1>
    <p>Widget Co reported third-quarter revenue of $1.2 billion, up 14 percent from a year
    earlier, driven by sustained demand for its industrial widget platform and a rebound in
    aftermarket services. Management raised full-year guidance to a range of $4.8 billion to
    $4.9 billion, citing a record backlog entering the fourth quarter.</p>
    <p>Gross margin expanded to 41 percent from 38 percent as input costs eased and the
    company's pricing actions carried through. The chief financial officer said free cash
    flow conversion should exceed 90 percent for the full year, funding the expanded
    buyback authorization announced alongside the results.</p>
    <p>Analysts had expected revenue of $1.15 billion and were mostly focused on the order
    book, where bookings grew 21 percent. The company flagged continued softness in its
    consumer segment, which it expects to bottom in the first half of next year, and said
    tariffs remain a manageable headwind at current rates.</p></article>
    <footer>© Widget Wire</footer></body></html>"#;

    #[test]
    fn fetch_extracts_an_article_over_the_wire() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "text/html; charset=utf-8")],
            body: ARTICLE_HTML,
        }]);
        let fetcher = HttpPageFetcher::allowing_loopback();
        let page = fetcher
            .fetch(&format!("{}article", server.base_url))
            .expect("fetch succeeds");
        assert_eq!(server.attempts(), 1);
        assert!(page.text.contains("$1.2 billion"), "{}", page.text);
        assert!(
            !page.text.contains("Home"),
            "navigation chrome is stripped: {}",
            page.text
        );
        assert!(page.extraction_quality > 0.2);
        assert!(!page.retrieved_at.is_empty());
    }

    #[test]
    fn a_thin_stub_is_flagged_not_errored() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "text/html")],
            body: "<html><body><p>Subscribe to continue reading.</p></body></html>",
        }]);
        let fetcher = HttpPageFetcher::allowing_loopback();
        let page = fetcher
            .fetch(&format!("{}stub", server.base_url))
            .expect("a thin page still fetches");
        assert!(page.thin_stub, "under-threshold body flags thin");
        assert!(page.extraction_quality < 0.3);
    }

    #[test]
    fn non_html_content_is_bounded_out() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/octet-stream")],
            body: "binary-ish",
        }]);
        let fetcher = HttpPageFetcher::allowing_loopback();
        let err = fetcher
            .fetch(&format!("{}bin", server.base_url))
            .unwrap_err()
            .to_string();
        assert!(err.contains("unsupported content type"), "{err}");
    }

    #[test]
    fn redirects_are_followed_and_revalidated_up_to_the_cap() {
        // One redirect hop to a same-host path, then the article.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 301,
                headers: vec![("Location", "/moved"), ("Content-Type", "text/html")],
                body: "",
            },
            Canned::Reply {
                status: 200,
                headers: vec![("Content-Type", "text/html")],
                body: ARTICLE_HTML,
            },
        ]);
        let fetcher = HttpPageFetcher::allowing_loopback();
        let page = fetcher
            .fetch(&format!("{}start", server.base_url))
            .expect("redirected fetch succeeds");
        assert_eq!(server.attempts(), 2);
        assert!(page.final_url.ends_with("/moved"));
    }

    #[test]
    fn error_statuses_surface_as_errors() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 403,
            headers: vec![("Content-Type", "text/html")],
            body: "forbidden",
        }]);
        let fetcher = HttpPageFetcher::allowing_loopback();
        let err = fetcher
            .fetch(&format!("{}nope", server.base_url))
            .unwrap_err()
            .to_string();
        assert!(err.contains("HTTP 403"), "{err}");
    }

    #[test]
    fn quality_reads_are_deterministic() {
        // Under the stub threshold: thin regardless of the readability gate.
        let (q, thin) = quality_of(120, true);
        assert!(thin);
        assert!(q < 0.05);
        // A full body: not thin, quality clamps at 1.
        let (q, thin) = quality_of(10_000, true);
        assert!(!thin);
        assert_eq!(q, 1.0);
        // A readability-gate failure is thin even when long.
        let (_, thin) = quality_of(10_000, false);
        assert!(thin);
    }
}
