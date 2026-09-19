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
//! SEC hosts use the declared identity shared with the EDGAR adapter; other
//! hosts use a realistic, browser-like header set — cheap
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

/// Why a live fetch failed, typed so the per-domain telemetry counts what the
/// *source* did without parsing error text (`docs/web-research.md §Extraction
/// telemetry`). `Policy` is the app's own guard — scheme, deny list, a
/// non-public address — which never reaches the network and is never the
/// source's record; `Http` is the source's own answer (401/403 read as
/// denied). Content and redirect bounds are deterministic failures; unmarked
/// transport errors retain their original typed source for retry classification.
/// Read back through [`failure_of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchFailure {
    Policy,
    Http(u16),
    Deterministic,
}

impl std::fmt::Display for FetchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Policy => f.write_str("blocked by the fetch policy"),
            Self::Http(status) => write!(f, "HTTP {status}"),
            Self::Deterministic => f.write_str("unusable document or redirect"),
        }
    }
}

impl std::error::Error for FetchFailure {}

/// The typed failure at the root of a fetch error's chain, if any.
pub fn failure_of(err: &anyhow::Error) -> Option<FetchFailure> {
    err.downcast_ref::<FetchFailure>().copied().or_else(|| {
        err.chain()
            .find_map(|cause| cause.downcast_ref::<FetchFailure>().copied())
    })
}

/// Ephemeral failure provenance, separate from requested-host telemetry.
#[derive(Debug)]
pub struct FetchLocation {
    pub url: String,
    pub retry_after: Option<Duration>,
    pub attempted: bool,
    pub(crate) message: String,
    pub(crate) detail: String,
}

impl std::fmt::Display for FetchLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FetchLocation {}

pub fn location_of(err: &anyhow::Error) -> Option<&FetchLocation> {
    err.downcast_ref::<FetchLocation>()
}

/// Positive typed evidence only: an opaque connect/DNS/TLS failure is unknown.
pub fn transient_failure(err: &anyhow::Error) -> bool {
    if let Some(class) = failure_of(err) {
        return matches!(class, FetchFailure::Http(408 | 429 | 500 | 502 | 503 | 504));
    }
    err.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(|e| e.is_timeout())
            || cause.downcast_ref::<std::io::Error>().is_some_and(|e| {
                matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                )
            })
    })
}

/// HTTP-date or decimal seconds. Overflow must never become an immediate retry.
fn retry_after(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    use chrono::Datelike;
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()) {
        return Some(Duration::from_secs(
            value.parse::<u64>().unwrap_or(u64::MAX),
        ));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|v| v.with_timezone(&chrono::Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(
                value.split_once(", ")?.1,
                "%d-%b-%y %H:%M:%S GMT",
            )
            .ok()
            .and_then(|date| {
                // RFC 850's two-digit year uses the most recent matching year
                // no more than fifty years in the future, not chrono's pivot.
                let mut year = now.year().div_euclid(100) * 100 + date.year().rem_euclid(100);
                if year < now.year() - 49 {
                    year += 100;
                }
                if year > now.year() + 50 {
                    year -= 100;
                }
                date.with_year(year).map(|v| v.and_utc())
            })
        })
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(value, "%a %b %e %H:%M:%S %Y")
                .ok()
                .map(|v| v.and_utc())
        });
    date.map(|date| {
        date.signed_duration_since(now)
            .to_std()
            .unwrap_or(Duration::ZERO)
    })
}

fn policy_err(message: String) -> anyhow::Error {
    anyhow::Error::new(FetchFailure::Policy).context(message)
}

/// The normalized requested host remains the failed-attempt telemetry key.
/// Redirect-destination provenance governs ephemeral backoff separately;
/// it does not change this persisted attribution. `None` for an invalid URL.
pub fn requested_host(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(registry::normalize_host(host))
}

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

/// Select from the current hop's parsed host, never a substring of the URL.
/// A terminal DNS root dot does not change the destination's identity.
fn user_agent_for(url: &Url) -> &'static str {
    let host = url.host_str().unwrap_or_default().trim_end_matches('.');
    if host == "sec.gov" || host.ends_with(".sec.gov") {
        crate::sec::SEC_USER_AGENT
    } else {
        USER_AGENT
    }
}

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

    /// The run's host cooldown binds on redirect destinations too.
    fn fetch_guarded(&self, url: &str, guard: &dyn Fn(&Url) -> Result<()>) -> Result<FetchedPage> {
        guard(&Url::parse(url)?)?;
        self.fetch(url)
    }
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

/// The URL's host as a literal address — IPv4 or bracketed IPv6 — read from
/// the parser's typed host rather than `host_str()`'s text (whose bracketed
/// IPv6 form is not a resolvable name). `None` for a domain name. The one
/// literal-host read both guards share (ruled 2026-08-28).
fn literal_host(url: &Url) -> Option<IpAddr> {
    match url.host()? {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        url::Host::Domain(_) => None,
    }
}

/// Resolve a URL's host and validate every address against the public-host
/// rules, returning the validated socket addresses so the connection can be
/// pinned to exactly what was checked (no second DNS answer). A literal
/// address resolves as itself with no lookup, so a public literal fetches and
/// a non-public one fails closed with its policy reason rather than a resolver
/// error (`docs/web-research.md §Safety and provenance`).
fn resolve_public(url: &Url, allow_loopback: bool) -> Result<Vec<SocketAddr>> {
    let host = url
        .host_str()
        .context("fetch URL carries no host")?
        .to_string();
    let port = url
        .port_or_known_default()
        .context("fetch URL has no usable port")?;
    let addrs: Vec<SocketAddr> = match literal_host(url) {
        Some(ip) => vec![SocketAddr::new(ip, port)],
        None => (host.as_str(), port)
            .to_socket_addrs()
            .with_context(|| format!("resolving {host}"))?
            .collect(),
    };
    if addrs.is_empty() {
        bail!("{host} resolved to no addresses");
    }
    for addr in &addrs {
        if allow_loopback && addr.ip().is_loopback() {
            continue;
        }
        if let Some(reason) = non_public_reason(addr.ip()) {
            return Err(policy_err(format!(
                "fetch blocked: {host} resolves to a {reason}"
            )));
        }
    }
    Ok(addrs)
}

/// Validate a URL's scheme and host per the SSRF rules. Returns the pinned
/// addresses on success.
fn validate_url(url: &Url, allow_loopback: bool) -> Result<Vec<SocketAddr>> {
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(policy_err(format!(
                "fetch blocked: scheme {other:?} is not allowed"
            )))
        }
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
        other => {
            return Err(policy_err(format!(
                "blocked: scheme {other:?} is not allowed"
            )))
        }
    }
    let host = url.host_str().context("URL carries no host")?;
    if let SourcePolicy::Deny(reason) = registry::assess(host) {
        return Err(policy_err(format!(
            "blocked: {host} is on the deny list ({reason})"
        )));
    }
    if let Some(reason) = literal_host(&url).and_then(non_public_reason) {
        return Err(policy_err(format!("blocked: {host} is a {reason}")));
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
    #[cfg(test)]
    test_address: Option<SocketAddr>,
}

impl HttpPageFetcher {
    pub fn new() -> Self {
        Self {
            allow_loopback: false,
            #[cfg(test)]
            test_address: None,
        }
    }

    /// Allow loopback targets so tests can drive the full fetch path against
    /// `test_http::MockHttp`. Test-only by construction.
    #[cfg(test)]
    pub fn allowing_loopback() -> Self {
        Self {
            allow_loopback: true,
            test_address: None,
        }
    }

    /// Keep the URL's host on the wire while using a local mock, without DNS.
    #[cfg(test)]
    pub(crate) fn with_test_address(address: SocketAddr) -> Self {
        assert!(address.ip().is_loopback());
        Self {
            allow_loopback: true,
            test_address: Some(address),
        }
    }

    fn resolve_url(&self, url: &Url) -> Result<Vec<SocketAddr>> {
        #[cfg(test)]
        if let Some(address) = self.test_address {
            // Only DNS/address validation is replaced. Keep URL policy and the
            // caller's per-hop guard active even on this test transport.
            check_url_policy(url.as_str())?;
            return Ok(vec![address]);
        }
        validate_url(url, self.allow_loopback)
    }

    /// One validated GET, redirects handled manually so every hop re-passes
    /// the SSRF rules. Returns the final URL and the (bounded) body text.
    fn get_bounded(
        &self,
        start: &Url,
        guard: &dyn Fn(&Url) -> Result<()>,
    ) -> Result<(Url, String)> {
        let mut url = start.clone();
        let mut attempted = false;
        for hop in 0..=REDIRECT_CAP {
            let mut retry_delay = None;
            // Keep provenance at the failing hop without replacing the source chain.
            let result: Result<Option<String>> = (|| {
                let host = url.host_str().unwrap_or_default();
                if let SourcePolicy::Deny(reason) = registry::assess(host) {
                    return Err(policy_err(format!(
                        "fetch blocked: {host} is on the deny list ({reason})"
                    )));
                }
                guard(&url)?;
                let addrs = self.resolve_url(&url).inspect_err(|err| {
                    // DNS failure is an admitted attempt; an app policy refusal is not.
                    attempted |= failure_of(err) != Some(FetchFailure::Policy);
                })?;
                let client = reqwest::blocking::Client::builder()
                    .timeout(FETCH_TIMEOUT)
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .resolve_to_addrs(host, &addrs)
                    .user_agent(user_agent_for(&url));
                // A test override must not route an SEC-named URL through an
                // ambient proxy. Production retains its existing proxy behavior.
                #[cfg(test)]
                let client = if self.test_address.is_some() {
                    client.no_proxy()
                } else {
                    client
                };
                let client = client
                    .build()
                    .context("building the fetch client")?;
                attempted = true;
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
                retry_delay = resp
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| retry_after(v, chrono::Utc::now()));
                if status.is_redirection() {
                    if hop == REDIRECT_CAP {
                        return Err(anyhow::Error::new(FetchFailure::Deterministic).context(
                            format!("fetch of {start} exceeded the {REDIRECT_CAP}-redirect cap"),
                        ));
                    }
                    let location = resp
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                        .ok_or_else(|| {
                            anyhow::Error::new(FetchFailure::Deterministic)
                                .context("redirect carried no Location header")
                        })?;
                    url = url.join(location).map_err(|err| {
                        anyhow::Error::new(err)
                            .context(FetchFailure::Deterministic)
                            .context(format!("joining redirect target {location:?}"))
                    })?;
                    return Ok(None);
                }
                if !status.is_success() {
                    return Err(anyhow::Error::new(FetchFailure::Http(status.as_u16()))
                        .context(format!("fetch of {url} returned HTTP {}", status.as_u16())));
                }
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
                    return Err(
                        anyhow::Error::new(FetchFailure::Deterministic).context(format!(
                            "fetch of {url} returned unsupported content type {content_type:?}"
                        )),
                    );
                }
                use std::io::Read;
                let mut body = Vec::new();
                resp.take(MAX_FETCH_BYTES)
                    .read_to_end(&mut body)
                    .with_context(|| format!("reading the body of {url}"))?;
                Ok(Some(String::from_utf8_lossy(&body).into_owned()))
            })();
            match result {
                Ok(Some(body)) => return Ok((url, body)),
                Ok(None) => continue,
                Err(err) => {
                    let location = FetchLocation {
                        url: url.to_string(),
                        retry_after: retry_delay,
                        attempted,
                        message: err.to_string(),
                        detail: format!("{err:#}"),
                    };
                    return Err(err.context(location));
                }
            }
        }
        unreachable!("last redirect returns the cap failure")
    }
}

impl Default for HttpPageFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl PageFetcher for HttpPageFetcher {
    fn fetch(&self, url: &str) -> Result<FetchedPage> {
        self.fetch_guarded(url, &|_| Ok(()))
    }

    fn fetch_guarded(&self, url: &str, guard: &dyn Fn(&Url) -> Result<()>) -> Result<FetchedPage> {
        let parsed = Url::parse(url).with_context(|| format!("parsing fetch URL {url:?}"))?;
        let (final_url, html) = self.get_bounded(&parsed, guard)?;
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

/// Local wire fixtures shared with the production research-runner tests.
#[cfg(test)]
pub(crate) mod test_support {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub(crate) struct WireServer {
        pub address: SocketAddr,
        requests: Arc<Mutex<Vec<String>>>,
    }

    impl WireServer {
        pub fn serve(responses: impl FnOnce(u16) -> Vec<String>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind local wire fixture");
            let address = listener.local_addr().unwrap();
            let responses = responses(address.port());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let recorded = requests.clone();
            std::thread::spawn(move || {
                for response in responses {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut head = Vec::new();
                    let mut byte = [0];
                    while head.len() < 65_536 && !head.ends_with(b"\r\n\r\n") {
                        if stream.read(&mut byte).unwrap_or(0) == 0 {
                            break;
                        }
                        head.push(byte[0]);
                    }
                    recorded
                        .lock()
                        .unwrap()
                        .push(String::from_utf8(head).unwrap());
                    let _ = stream.write_all(response.as_bytes());
                }
            });
            Self { address, requests }
        }

        pub fn url(&self, host: &str, path: &str) -> String {
            format!("http://{host}:{}{path}", self.address.port())
        }

        pub fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    pub(crate) fn response(status: u16, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::{Canned, MockHttp};

    #[test]
    fn entry6_sec_identity_matches_only_sec_hosts() {
        for host in [
            "sec.gov",
            "www.sec.gov",
            "data.sec.gov",
            "WWW.SEC.GOV",
            "sec.gov.",
            "DATA.SEC.GOV.",
        ] {
            let url = Url::parse(&format!("https://{host}/Archives/example")).unwrap();
            assert_eq!(user_agent_for(&url), crate::sec::SEC_USER_AGENT, "{host}");
        }
        for url in [
            "https://notsec.gov/",
            "https://sec.gov.example/",
            "https://example.com/sec.gov",
            "https://sec.gov@example.com/",
            "https://example.com/?host=sec.gov",
            "http://8.8.8.8/",
        ] {
            assert_eq!(
                user_agent_for(&Url::parse(url).unwrap()),
                USER_AGENT,
                "{url}"
            );
        }
    }

    #[test]
    fn entry6_wire_identity_follows_each_redirect_destination() {
        use test_support::{response, WireServer};
        let server = WireServer::serve(|port| {
            vec![
                response(
                    302,
                    &format!("Location: http://www.sec.gov:{port}/filing\r\n"),
                    "",
                ),
                response(
                    302,
                    &format!("Location: http://publisher.example:{port}/article\r\n"),
                    "",
                ),
                response(200, "Content-Type: text/plain\r\n", "Published text"),
            ]
        });
        let page = HttpPageFetcher::with_test_address(server.address)
            .fetch(&server.url("publisher.example", "/start"))
            .unwrap();
        assert_eq!(page.final_url, server.url("publisher.example", "/article"));
        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        for (request, expected) in
            requests
                .iter()
                .zip([USER_AGENT, crate::sec::SEC_USER_AGENT, USER_AGENT])
        {
            let ua = request.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("user-agent")
                    .then(|| value.trim())
            });
            assert_eq!(ua, Some(expected));
            assert!(request
                .to_ascii_lowercase()
                .contains("accept-language: en-us,en;q=0.9"));
        }
        assert!(requests[1].contains(&format!("host: www.sec.gov:{}", server.address.port())));
    }

    #[test]
    fn entry6_declared_identity_does_not_reclassify_a_wire_denial() {
        use test_support::{response, WireServer};
        let server = WireServer::serve(|_| vec![response(403, "", "Denied")]);
        let err = HttpPageFetcher::with_test_address(server.address)
            .fetch(&server.url("sec.gov", "/filing"))
            .unwrap_err();
        assert_eq!(failure_of(&err), Some(FetchFailure::Http(403)));
        assert!(!transient_failure(&err));
        assert!(location_of(&err).unwrap().detail.contains("HTTP 403"));
        assert!(server.requests()[0].contains(crate::sec::SEC_USER_AGENT));
        assert_eq!(server.requests().len(), 1);
    }

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
            (
                IpAddr::V4(Ipv4Addr::new(192, 0, 0, 170)),
                "protocol-assignment",
            ),
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
        assert_eq!(
            non_public_reason("2606:2800:220:1::1".parse().unwrap()),
            None
        );
    }

    #[test]
    fn cache_url_policy_precheck_blocks_denied_and_literal_addresses() {
        // The no-network policy check a cache read must pass: scheme, deny
        // list, literal-address classes — so an imported or legacy cache row
        // can't serve under a policy the current rules would block.
        assert!(check_url_policy("https://reuters.com/a").is_ok());
        let err = check_url_policy("ftp://reuters.com/a")
            .unwrap_err()
            .to_string();
        assert!(err.contains("scheme"), "{err}");
        let err = check_url_policy("https://stockinvest.us/x")
            .unwrap_err()
            .to_string();
        assert!(err.contains("deny list"), "{err}");
        let err = check_url_policy("http://192.168.1.10/admin")
            .unwrap_err()
            .to_string();
        assert!(err.contains("private"), "{err}");
        let err = check_url_policy("http://[fec0::1]/x")
            .unwrap_err()
            .to_string();
        assert!(err.contains("site-local"), "{err}");
    }

    #[test]
    fn literal_hosts_validate_as_themselves_without_a_lookup() {
        // A literal address resolves as itself — `host_str()`'s bracketed
        // IPv6 form is not a resolvable name, and the first cut handed it to
        // the resolver, so no IPv6-literal URL could ever fetch. A public
        // literal pins to exactly itself; a non-public one fails closed with
        // its policy reason, not a resolver error. No DNS is touched.
        let pinned = |u: &str| validate_url(&Url::parse(u).unwrap(), false);
        assert_eq!(
            pinned("http://[2606:2800:220:1::1]/x").unwrap(),
            vec!["[2606:2800:220:1::1]:80".parse::<SocketAddr>().unwrap()]
        );
        assert_eq!(
            pinned("https://93.184.216.34/x").unwrap(),
            vec!["93.184.216.34:443".parse::<SocketAddr>().unwrap()]
        );
        for (url, needle) in [
            ("http://[::1]/x", "loopback"),
            ("http://[fec0::1]/x", "site-local"),
            ("http://[::ffff:192.168.1.1]/x", "private"),
            ("http://10.0.0.7/x", "private"),
        ] {
            let err = pinned(url).unwrap_err().to_string();
            assert!(err.contains(needle), "{url}: {err}");
            assert!(!err.contains("resolving"), "{url}: {err}");
        }
    }

    #[test]
    fn production_guard_blocks_loopback_targets() {
        // The production fetcher (no test allowance) refuses a loopback URL —
        // the rule that protects the app's own Ollama / SearXNG — in both
        // literal forms.
        let fetcher = HttpPageFetcher::new();
        for url in ["http://127.0.0.1:9/never", "http://[::1]:9/never"] {
            let err = fetcher.fetch(url).unwrap_err().to_string();
            assert!(err.contains("loopback"), "{url}: {err}");
        }
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
            .unwrap_err();
        // The status rides the chain typed, so the per-domain telemetry can
        // count a denied answer without parsing the message.
        assert_eq!(failure_of(&err), Some(FetchFailure::Http(403)), "{err:#}");
        let err = err.to_string();
        assert!(err.contains("HTTP 403"), "{err}");
    }

    #[test]
    fn policy_refusals_are_typed_and_the_requested_host_keys_a_failure() {
        // The app's own guard marks itself `Policy` so a refusal is never
        // charged to the source's telemetry; the requested host is the key a
        // failed attempt (no page, no post-redirect host) can still name.
        let err = check_url_policy("https://stockinvest.us/x").unwrap_err();
        assert_eq!(failure_of(&err), Some(FetchFailure::Policy), "{err:#}");
        let err = check_url_policy("http://192.168.1.10/admin").unwrap_err();
        assert_eq!(failure_of(&err), Some(FetchFailure::Policy), "{err:#}");
        let fetcher = HttpPageFetcher::allowing_loopback();
        let err = fetcher.fetch("ftp://example.com/x").unwrap_err();
        assert_eq!(failure_of(&err), Some(FetchFailure::Policy), "{err:#}");
        assert_eq!(failure_of(&anyhow::anyhow!("connection reset")), None);
        assert_eq!(
            requested_host("https://www.Reuters.com/business/widget").as_deref(),
            Some("reuters.com")
        );
        assert_eq!(requested_host("not a url"), None);
    }

    #[test]
    fn entry4_retry_classifier_uses_status_and_typed_transport_evidence() {
        for status in [408, 429, 500, 502, 503, 504] {
            assert!(transient_failure(&anyhow::Error::new(FetchFailure::Http(
                status
            ))));
        }
        for status in [400, 401, 403, 404, 410, 501, 505] {
            assert!(!transient_failure(&anyhow::Error::new(FetchFailure::Http(
                status
            ))));
        }
        for kind in [
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::BrokenPipe,
        ] {
            assert!(transient_failure(
                &anyhow::Error::new(std::io::Error::from(kind)).context("fetching")
            ));
        }
        for kind in [
            std::io::ErrorKind::InvalidData,
            std::io::ErrorKind::ConnectionRefused,
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::UnexpectedEof,
        ] {
            assert!(!transient_failure(&anyhow::Error::new(
                std::io::Error::from(kind)
            )));
        }
        assert!(!transient_failure(&anyhow::anyhow!(
            "TLS timeout, DNS connection reset"
        )));
        let policy = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::TimedOut))
            .context(FetchFailure::Policy);
        assert!(
            !transient_failure(&policy),
            "policy refusal overrides a transport cause"
        );
    }

    #[test]
    fn entry4_retry_after_dates_seconds_and_invalid_values() {
        let now = chrono::DateTime::parse_from_rfc3339("1994-11-06T08:49:30Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        for value in [
            "7",
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
        ] {
            assert_eq!(
                retry_after(value, now),
                Some(Duration::from_secs(7)),
                "{value}"
            );
        }
        assert_eq!(retry_after("0", now), Some(Duration::ZERO));
        assert_eq!(
            retry_after("Sun, 06 Nov 1994 08:49:20 GMT", now),
            Some(Duration::ZERO)
        );
        for value in ["-1", "no date", "1.5", ""] {
            assert_eq!(retry_after(value, now), None);
        }
        assert_eq!(
            retry_after("999999999999999999999999999999999999", now),
            Some(Duration::from_secs(u64::MAX))
        );
    }

    #[test]
    fn entry4_wire_failure_keeps_status_retry_after_and_failing_redirect_url() {
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 302,
                headers: vec![("Location", "/destination")],
                body: "",
            },
            Canned::Reply {
                status: 503,
                headers: vec![("Retry-After", "7")],
                body: "unavailable",
            },
        ]);
        let err = HttpPageFetcher::allowing_loopback()
            .fetch(&format!("{}start", server.base_url))
            .unwrap_err();
        assert_eq!(
            server.attempts(),
            2,
            "redirect hops, no hidden application retry"
        );
        assert_eq!(failure_of(&err), Some(FetchFailure::Http(503)));
        let location = location_of(&err).unwrap();
        assert!(location.url.ends_with("/destination"));
        assert_eq!(location.retry_after, Some(Duration::from_secs(7)));
        assert!(location.attempted);
    }

    #[test]
    fn entry4_redirect_guard_runs_before_contacting_destination() {
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 302,
            headers: vec![("Location", "https://denied.example/article")],
            body: "",
        }]);
        let guard = |url: &Url| {
            if url.host_str() == Some("denied.example") {
                bail!("host cooling down");
            }
            Ok(())
        };
        let fetcher = HttpPageFetcher::allowing_loopback();
        let err = fetcher.fetch_guarded(&server.base_url, &guard).unwrap_err();
        assert_eq!(server.attempts(), 1);
        assert_eq!(
            location_of(&err).unwrap().url,
            "https://denied.example/article"
        );
        assert!(
            location_of(&err).unwrap().attempted,
            "earlier redirect work still spent one attempt"
        );
        let err = fetcher
            .fetch_guarded("https://denied.example/article", &guard)
            .unwrap_err();
        assert!(
            !location_of(&err).unwrap().attempted,
            "a direct skip spent nothing"
        );
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
