//! Schwab three-legged OAuth 2.0 (`docs/schwab-integration.md §Authorization`,
//! §Token lifecycle): the browser login redirects to a self-signed HTTPS loopback
//! server that captures the authorization code; the token pair lives on the Keychain
//! rail ([`crate::schwab_secrets`]) and [`OauthClient::valid_access_token`] refreshes
//! it until the 7-day refresh window lapses ([`ReauthRequired`]).
//!
//! Access and refresh tokens are never logged and never emitted on the progress seam:
//! only endpoint and status ever reach an error message, never a token or the success
//! body.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{ValidationReport, WarningCategory, WarningKind};
use crate::loopback_https::LoopbackHttpsServer;
use crate::schwab_secrets::{SchwabTokens, TokenStore, SECRET_CLIENT_SECRET};

/// Schwab's authorization endpoint (leg 1 — the browser login URL).
const AUTHORIZE_URL: &str = "https://api.schwabapi.com/v1/oauth/authorize";
/// Schwab's token endpoint (legs 2 & 3 — code exchange and refresh).
const TOKEN_URL: &str = "https://api.schwabapi.com/v1/oauth/token";
/// The registered callback. Exact-match and HTTPS-only per the developer app
/// (`docs/schwab-integration.md`) — no trailing slash.
pub const REDIRECT_URI: &str = "https://127.0.0.1:8182";
/// The loopback address the capture server binds.
pub const LOOPBACK_ADDR: &str = "127.0.0.1:8182";

/// Access-token safety margin: refresh a token this close to expiry rather than risk
/// it lapsing mid-request.
const ACCESS_SKEW: Duration = Duration::seconds(60);
/// Refresh-token lifetime — 7 days, and it cannot be extended
/// (`docs/schwab-integration.md §Token lifecycle`).
const REFRESH_LIFETIME_DAYS: i64 = 7;
/// How long the loopback capture waits for the browser redirect, so an abandoned login
/// can't park the capture thread (and the run slot) indefinitely. Fully-qualified to
/// avoid shadowing chrono's `Duration`, which the token-lifecycle math uses.
const CAPTURE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// No stored tokens, or the refresh token has lapsed: only another interactive browser
/// login recovers. A distinct type so the run-gate can tell "reconnect Schwab" apart
/// from a transient network error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReauthRequired;

impl std::fmt::Display for ReauthRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Schwab re-authentication required — connect your Schwab account to continue"
        )
    }
}

impl std::error::Error for ReauthRequired {}

/// The leg-1 browser authorization URL. `state` is a per-run nonce the redirect must
/// echo back (a CSRF guard).
pub fn authorize_url(client_id: &str, state: &str) -> String {
    let redirect = encode_component(REDIRECT_URI);
    let client = encode_component(client_id);
    let state = encode_component(state);
    format!("{AUTHORIZE_URL}?response_type=code&client_id={client}&redirect_uri={redirect}&state={state}")
}

/// Percent-encodes everything outside the RFC-3986 unreserved set. Kept local so
/// building the authorize URL needs no extra dependency.
fn encode_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Extract the one-time authorization `code` from the loopback redirect target (the
/// path+query the capture server hands back, e.g. `/?code=ABC&session=XYZ`).
pub fn parse_redirect_code(target: &str) -> Result<String> {
    // A relative target is resolved against a throwaway base so the standard
    // query-pair decoder applies.
    let base = reqwest::Url::parse("http://127.0.0.1/").expect("static base URL is valid");
    // The target stays out of the error: a malformed but code-bearing redirect would
    // otherwise put the one-time code into a string a caller could surface.
    let parsed = base
        .join(target)
        .context("parsing OAuth redirect target")?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .filter(|c| !c.is_empty())
        .ok_or_else(|| anyhow!("OAuth redirect carried no authorization code"))
}

/// The `state` nonce echoed on the redirect, if present.
fn redirect_state(target: &str) -> Option<String> {
    let base = reqwest::Url::parse("http://127.0.0.1/").expect("static base URL is valid");
    let parsed = base.join(target).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
}

/// Schwab's token-endpoint success body; extra fields (`token_type`, `scope`,
/// `id_token`) are ignored.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    /// Access-token lifetime in seconds (Schwab sends 1800).
    expires_in: i64,
}

/// Redacting `Debug`: a derived one would hand any `{:?}` site the raw token pair.
impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// Cap on how much of a token-endpoint error body is surfaced. A well-formed OAuth
/// error body fits comfortably; the cap only bites on a misbehaving provider or proxy.
const ERROR_BODY_CAP_CHARS: usize = 300;

/// Truncate an error body to [`ERROR_BODY_CAP_CHARS`] on a char boundary, marking the
/// cut.
fn cap_error_body(body: &str) -> String {
    let (head, cut) = crate::data_sources::cap_chars(body, ERROR_BODY_CAP_CHARS);
    if cut {
        format!("{head} …(truncated)")
    } else {
        head
    }
}

/// Exchanges authorization codes and hands out live access tokens, refreshing as
/// needed. `token_url` is a field so tests can point it at a localhost mock.
pub struct OauthClient {
    http: reqwest::blocking::Client,
    token_url: String,
    client_id: String,
    store: Arc<dyn TokenStore>,
}

impl OauthClient {
    /// Build against Schwab's real token endpoint.
    pub fn new(client_id: impl Into<String>, store: Arc<dyn TokenStore>) -> Result<Self> {
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .build()
                .context("building Schwab OAuth HTTP client")?,
            token_url: TOKEN_URL.to_string(),
            client_id: client_id.into(),
            store,
        })
    }

    /// Test seam: point the token calls at an arbitrary base URL (the localhost mock).
    #[cfg(test)]
    pub fn with_token_url(
        client_id: impl Into<String>,
        store: Arc<dyn TokenStore>,
        token_url: impl Into<String>,
    ) -> Self {
        Self {
            http: reqwest::blocking::Client::new(),
            token_url: token_url.into(),
            client_id: client_id.into(),
            store,
        }
    }

    /// The developer app secret for the token-endpoint Basic auth; absent means the
    /// connection was never configured.
    fn client_secret(&self) -> Result<String> {
        self.store
            .get(SECRET_CLIENT_SECRET)?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow!("Schwab client secret is not configured"))
    }

    /// Single-shot: an authorization code is single-use, so a blind retry could
    /// double-spend it. Never logs the body (it carries the tokens).
    fn post_token(&self, form: &[(&str, &str)]) -> Result<TokenResponse> {
        let secret = self.client_secret()?;
        let resp = self
            .http
            .post(&self.token_url)
            .basic_auth(&self.client_id, Some(&secret))
            .form(form)
            .send()
            .context("sending Schwab token request")?;
        let status = resp.status();
        if !status.is_success() {
            // The error body carries no token, so it is safe to surface; the success
            // body never is. Capped because the body is provider/proxy-controlled and
            // this string travels far (UI error, job history, run audit).
            let body = resp.text().unwrap_or_default();
            bail!(
                "Schwab token endpoint returned {status}: {}",
                cap_error_body(&body)
            );
        }
        resp.json::<TokenResponse>()
            .context("parsing Schwab token response")
    }

    /// Exchange a captured authorization `code` for the token set and persist it. The
    /// refresh window is anchored at `now` and never extended thereafter.
    pub fn exchange_code(&self, code: &str, now: DateTime<Utc>) -> Result<SchwabTokens> {
        let resp = self.post_token(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
        ])?;
        let tokens = SchwabTokens {
            access_token: resp.access_token,
            refresh_token: resp.refresh_token,
            access_expires_at: now + Duration::seconds(resp.expires_in),
            refresh_expires_at: now + Duration::days(REFRESH_LIFETIME_DAYS),
        };
        self.store.set_tokens(&tokens)?;
        Ok(tokens)
    }

    /// The 7-day refresh window is not reset by a refresh, so a weekly re-login is
    /// unavoidable even under continuous use. [`ReauthRequired`] when there is nothing
    /// to refresh or the window has lapsed.
    pub fn refresh(&self, now: DateTime<Utc>) -> Result<SchwabTokens> {
        let current = self.store.tokens()?.ok_or(ReauthRequired)?;
        if !current.refresh_valid(now) {
            return Err(ReauthRequired.into());
        }
        let resp = self.post_token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &current.refresh_token),
        ])?;
        let tokens = SchwabTokens {
            access_token: resp.access_token,
            // Adopt Schwab's fresh refresh token but keep the original expiry: the
            // 7-day window cannot be extended.
            refresh_token: resp.refresh_token,
            access_expires_at: now + Duration::seconds(resp.expires_in),
            refresh_expires_at: current.refresh_expires_at,
        };
        self.store.set_tokens(&tokens)?;
        Ok(tokens)
    }

    /// The stored access token when still valid, otherwise a transparent refresh.
    /// [`ReauthRequired`] when not connected or the refresh window has lapsed.
    pub fn valid_access_token(&self, now: DateTime<Utc>) -> Result<String> {
        let current = self.store.tokens()?.ok_or(ReauthRequired)?;
        if !current.refresh_valid(now) {
            return Err(ReauthRequired.into());
        }
        if current.access_valid(now, ACCESS_SKEW) {
            return Ok(current.access_token);
        }
        Ok(self.refresh(now)?.access_token)
    }
}

/// Whether the Keychain rail holds a usable connection, with no network call. A failed
/// read of the token set is its own state rather than an error, so the gate can say
/// why the local jobs are blocked (`docs/schwab-integration.md §Failure posture`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchwabPresence {
    /// Tokens present and the refresh window open.
    Connected,
    /// No token set stored, or the 7-day refresh window has lapsed.
    NotConnected,
    /// The stored token set could not be read.
    Unreadable,
}

/// Never fails: a read error is [`SchwabPresence::Unreadable`], its cause written to
/// stderr (the entry and the failure, never a stored value).
pub fn schwab_presence(store: &dyn TokenStore, now: DateTime<Utc>) -> SchwabPresence {
    match store.tokens() {
        Ok(Some(t)) if t.refresh_valid(now) => SchwabPresence::Connected,
        Ok(_) => SchwabPresence::NotConnected,
        Err(e) => {
            eprintln!("schwab connection unreadable: {e:#}");
            SchwabPresence::Unreadable
        }
    }
}

/// The connection state the Settings surface renders, derived from the stored token set
/// without a network call (`docs/interface.md §Connection status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchwabConnection {
    /// No token set stored.
    NotConnected,
    /// Tokens present and the refresh window still open.
    Connected,
    /// Tokens present but the refresh window has lapsed; only a fresh interactive login
    /// recovers it.
    Expired,
}

/// Mirrors [`schwab_presence`]'s refresh-window rule but keeps `Expired` distinct so
/// the surface can prompt re-login rather than first-connect.
pub fn connection_state(tokens: &Option<SchwabTokens>, now: DateTime<Utc>) -> SchwabConnection {
    match tokens {
        None => SchwabConnection::NotConnected,
        Some(t) if t.refresh_valid(now) => SchwabConnection::Connected,
        Some(_) => SchwabConnection::Expired,
    }
}

/// What the Settings Schwab-connection surface renders (`docs/interface.md §Settings`).
/// `client_id` is a non-secret identifier the form pre-fills; the client secret and the
/// tokens never cross this seam, only `secret_configured` and the derived state do.
/// `refresh_expires_at` drives the weekly-re-login heads-up.
#[derive(Debug, Clone, Serialize)]
pub struct SchwabStatus {
    pub client_id: String,
    pub secret_configured: bool,
    pub connection: SchwabConnection,
    pub refresh_expires_at: Option<DateTime<Utc>>,
}

impl SchwabStatus {
    pub fn build(
        client_id: String,
        secret_configured: bool,
        tokens: Option<SchwabTokens>,
        now: DateTime<Utc>,
    ) -> Self {
        let connection = connection_state(&tokens, now);
        let refresh_expires_at = tokens.map(|t| t.refresh_expires_at);
        Self {
            client_id,
            secret_configured,
            connection,
            refresh_expires_at,
        }
    }

    /// A failed read of the client secret is an error. A failed read of the token set
    /// reads as not connected, its cause written to stderr, so the Settings section still
    /// renders and a connect overwrites the stored set
    /// (`docs/schwab-integration.md §Failure posture`).
    pub fn read(client_id: String, store: &dyn TokenStore, now: DateTime<Utc>) -> Result<Self> {
        let secret_configured = store
            .get(SECRET_CLIENT_SECRET)?
            .is_some_and(|s| !s.trim().is_empty());
        let tokens = store.tokens().unwrap_or_else(|e| {
            eprintln!("schwab connection unreadable: {e:#}");
            None
        });
        Ok(Self::build(client_id, secret_configured, tokens, now))
    }
}

/// The [`schwab_gate`] item when no Schwab account is connected
/// (`docs/schwab-integration.md §A connected Schwab account is required`). Job-neutral
/// because the gate is shared by both local jobs.
pub const SCHWAB_NOT_CONNECTED_MSG: &str =
    "Schwab account not connected — connect your Schwab account (weekly re-login) to run this analysis.";

/// The [`schwab_gate`] item when the stored token set could not be read
/// (`docs/schwab-integration.md §Failure posture`).
pub const SCHWAB_UNREADABLE_MSG: &str =
    "Schwab connection unreadable — allow Market Signal to read its Keychain entry when macOS asks, or reconnect your Schwab account, to run this analysis.";

/// The local-suite Schwab-connection gate: one blocking [`WarningKind::Schwab`] category
/// unless connected, the shape `local_model::local_gate` produces for the daemon.
/// Independent of the cloud-report gate (`config::validate`): a missing or unreadable
/// connection blocks only the local jobs, never the Market Signal Report.
pub fn schwab_gate(presence: SchwabPresence) -> ValidationReport {
    let item = match presence {
        SchwabPresence::Connected => None,
        SchwabPresence::NotConnected => Some(SCHWAB_NOT_CONNECTED_MSG),
        SchwabPresence::Unreadable => Some(SCHWAB_UNREADABLE_MSG),
    };
    let mut categories = Vec::new();
    if let Some(item) = item {
        categories.push(WarningCategory {
            kind: WarningKind::Schwab,
            title: "Charles Schwab connection".to_string(),
            items: vec![item.to_string()],
            dismiss_id: None,
        });
    }
    let is_blocked = categories.iter().any(|c| c.kind.is_blocking());
    ValidationReport {
        categories,
        is_blocked,
    }
}

/// Stand up the self-signed HTTPS server, open the browser at the authorize URL, block
/// for the single redirect, and return its authorization code. `open_browser` is false
/// only in the `#[ignore]` smoke, which drives the redirect by hand.
pub fn run_loopback_capture(client_id: &str, open_browser: bool) -> Result<String> {
    // No CA signs a loopback address, so the cert is self-signed and the browser shows
    // a one-time warning the user clicks through (`docs/schwab-integration.md`).
    // Generated fresh per run, in memory only; `serialize_der` is PKCS#8.
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])
        .context("generating self-signed loopback certificate")?;
    let cert = certified.cert.der().clone();
    let key =
        rustls::pki_types::PrivateKeyDer::Pkcs8(certified.signing_key.serialize_der().into());
    let server = LoopbackHttpsServer::bind(LOOPBACK_ADDR, cert, key)?;

    // A redirect that doesn't echo this exact nonce is not our login, and is rejected.
    let state = uuid::Uuid::new_v4().to_string();
    if open_browser {
        open_url(&authorize_url(client_id, &state));
    }

    capture_redirect(&server, &state, Instant::now() + CAPTURE_TIMEOUT)
}

/// Wait for the redirect that carries a code and echoes exactly `state`. A code-less
/// probe (a browser prefetch, a favicon) is answered and ignored; a code-bearing
/// request with the wrong state aborts the whole capture (fail-closed: one guess, no
/// retries against the nonce).
fn capture_redirect(
    server: &LoopbackHttpsServer,
    state: &str,
    deadline: Instant,
) -> Result<String> {
    loop {
        let Some(request) = server.next_request(deadline)? else {
            bail!("timed out waiting for the Schwab authorization redirect");
        };
        match parse_redirect_code(request.target()) {
            Ok(code) => {
                if redirect_state(request.target()).as_deref() != Some(state) {
                    request.respond("State mismatch — login attempt aborted.");
                    bail!("OAuth redirect state did not match the issued nonce; aborting");
                }
                request.respond(CLOSE_TAB_HTML);
                return Ok(code);
            }
            Err(_) => request.respond("Waiting for Schwab…"),
        }
    }
}

/// The page shown in the browser once the code is captured.
const CLOSE_TAB_HTML: &str =
    "<html><body><h2>Schwab connected.</h2><p>You can close this tab and return to Market Signal.</p></body></html>";

/// Best-effort: on failure the user opens the URL themselves; the capture server is
/// already listening.
fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schwab_secrets::{DeniedTokenStore, InMemoryTokenStore, SECRET_TOKENS};
    use crate::test_http::{Canned, MockHttp};

    fn store_with_secret() -> Arc<InMemoryTokenStore> {
        let store = Arc::new(InMemoryTokenStore::new());
        store.set(SECRET_CLIENT_SECRET, "dev-secret").unwrap();
        store
    }

    fn client(store: Arc<InMemoryTokenStore>, url: &str) -> OauthClient {
        OauthClient::with_token_url("client-abc", store, url)
    }

    #[test]
    fn authorize_url_carries_code_flow_encoded_redirect_and_state() {
        let url = authorize_url("client-abc@AMER.OAUTHAP", "nonce-123");
        assert!(url.starts_with(AUTHORIZE_URL), "{url}");
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("redirect_uri=https%3A%2F%2F127.0.0.1%3A8182"), "{url}");
        assert!(url.contains("client_id=client-abc%40AMER.OAUTHAP"), "{url}");
        assert!(url.contains("state=nonce-123"), "{url}");
    }

    #[test]
    fn redirect_state_reads_the_echoed_nonce() {
        assert_eq!(
            redirect_state("/?code=ABC&state=nonce-123").as_deref(),
            Some("nonce-123")
        );
        assert_eq!(redirect_state("/?code=ABC").as_deref(), None);
    }

    #[test]
    fn parse_redirect_code_pulls_the_code_and_decodes_it() {
        assert_eq!(
            parse_redirect_code("/?code=ABC123&session=zzz").unwrap(),
            "ABC123"
        );
        assert_eq!(
            parse_redirect_code("/?code=C0%2FbXy%40z&session=q").unwrap(),
            "C0/bXy@z"
        );
    }

    #[test]
    fn parse_redirect_code_errors_when_no_code_present() {
        assert!(parse_redirect_code("/?error=access_denied").is_err());
        assert!(parse_redirect_code("/?code=").is_err());
    }

    #[test]
    fn parse_redirect_code_error_never_carries_the_code() {
        // The invalid IPv6 host fails the URL parse while the target still carries a
        // code; the full context chain must not leak it.
        let err = parse_redirect_code("http://[bad/?code=SECRET-CODE").unwrap_err();
        let chain = format!("{err:#}");
        assert!(!chain.contains("SECRET-CODE"), "{chain}");
    }

    #[test]
    fn token_endpoint_error_body_is_capped_in_the_surfaced_message() {
        let now = Utc::now();
        let store = store_with_secret();
        let huge: &'static str = Box::leak("x".repeat(5_000).into_boxed_str());
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 400,
            headers: vec![("Content-Type", "application/json")],
            body: huge,
        }]);
        let err = client(store, &server.base_url)
            .exchange_code("the-code", now)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("400"), "{msg}");
        assert!(msg.contains("…(truncated)"), "{msg}");
        assert!(msg.len() < 500, "capped message, got {} chars", msg.len());
    }

    #[test]
    fn token_response_debug_redacts_the_token_values() {
        let resp = TokenResponse {
            access_token: "acc-secret".into(),
            refresh_token: "ref-secret".into(),
            expires_in: 1800,
        };
        let dump = format!("{resp:?}");
        assert!(!dump.contains("acc-secret"), "{dump}");
        assert!(!dump.contains("ref-secret"), "{dump}");
        assert!(dump.contains("1800"), "{dump}");
    }

    #[test]
    fn exchange_code_parses_tokens_and_anchors_the_windows() {
        let now = Utc::now();
        let store = store_with_secret();
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"access_token":"acc-1","refresh_token":"ref-1","expires_in":1800,"token_type":"Bearer"}"#,
        }]);
        let toks = client(store.clone(), &server.base_url)
            .exchange_code("the-code", now)
            .expect("exchange succeeds");
        assert_eq!(toks.access_token, "acc-1");
        assert_eq!(toks.refresh_token, "ref-1");
        assert_eq!(toks.access_expires_at, now + Duration::seconds(1800));
        assert_eq!(toks.refresh_expires_at, now + Duration::days(7));
        assert_eq!(store.tokens().unwrap().unwrap(), toks);
    }

    #[test]
    fn valid_access_token_returns_the_stored_one_while_fresh() {
        let now = Utc::now();
        let store = store_with_secret();
        store
            .set_tokens(&SchwabTokens {
                access_token: "still-good".into(),
                refresh_token: "ref".into(),
                access_expires_at: now + Duration::minutes(20),
                refresh_expires_at: now + Duration::days(5),
            })
            .unwrap();
        // No mock server: a valid access token is returned without a network call.
        let tok = client(store, "http://127.0.0.1:1/unused")
            .valid_access_token(now)
            .unwrap();
        assert_eq!(tok, "still-good");
    }

    #[test]
    fn valid_access_token_refreshes_an_expired_access_token() {
        let now = Utc::now();
        let store = store_with_secret();
        store
            .set_tokens(&SchwabTokens {
                access_token: "stale".into(),
                refresh_token: "ref-old".into(),
                access_expires_at: now - Duration::minutes(1), // expired
                refresh_expires_at: now + Duration::days(3),    // window still open
            })
            .unwrap();
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"access_token":"fresh","refresh_token":"ref-new","expires_in":1800}"#,
        }]);
        let tok = client(store.clone(), &server.base_url)
            .valid_access_token(now)
            .expect("refresh reaches a fresh token");
        assert_eq!(tok, "fresh");
        // The refresh window is carried, not reset.
        let stored = store.tokens().unwrap().unwrap();
        assert_eq!(stored.refresh_expires_at, now + Duration::days(3));
        assert_eq!(stored.refresh_token, "ref-new");
    }

    #[test]
    fn valid_access_token_reauth_required_when_refresh_window_lapsed() {
        let now = Utc::now();
        let store = store_with_secret();
        store
            .set_tokens(&SchwabTokens {
                access_token: "stale".into(),
                refresh_token: "ref".into(),
                access_expires_at: now - Duration::minutes(1),
                refresh_expires_at: now - Duration::seconds(1), // lapsed
            })
            .unwrap();
        let err = client(store, "http://127.0.0.1:1/unused")
            .valid_access_token(now)
            .unwrap_err();
        assert!(err.downcast_ref::<ReauthRequired>().is_some(), "{err}");
    }

    #[test]
    fn valid_access_token_reauth_required_when_never_connected() {
        let now = Utc::now();
        let store = store_with_secret(); // secret set, but no tokens
        let err = client(store, "http://127.0.0.1:1/unused")
            .valid_access_token(now)
            .unwrap_err();
        assert!(err.downcast_ref::<ReauthRequired>().is_some(), "{err}");
    }

    // ---- Offline loopback-capture round-trips (real TLS, no browser) ----

    /// A capture on an ephemeral port with a known `state`: the bound address and the
    /// capture thread.
    fn spawn_capture(
        state: &str,
        deadline: std::time::Duration,
    ) -> (
        std::net::SocketAddr,
        std::thread::JoinHandle<Result<String>>,
    ) {
        let certified =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
        let cert = certified.cert.der().clone();
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            certified.signing_key.serialize_der().into(),
        );
        let server = LoopbackHttpsServer::bind("127.0.0.1:0", cert, key).unwrap();
        let addr = server.local_addr().unwrap();
        let state = state.to_string();
        let handle = std::thread::spawn(move || {
            capture_redirect(&server, &state, Instant::now() + deadline)
        });
        (addr, handle)
    }

    /// The capture's certificate is self-signed, so the client must skip verification
    /// to complete the handshake (the browser's click-through, in code).
    fn insecure_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap()
    }

    #[test]
    fn loopback_capture_returns_the_code_for_a_state_matching_redirect() {
        let (addr, handle) = spawn_capture("nonce-ok", std::time::Duration::from_secs(10));
        let resp = insecure_client()
            .get(format!("https://{addr}/?code=CAP-123&state=nonce-ok"))
            .send()
            .expect("redirect delivered");
        assert!(resp.text().unwrap().contains("Schwab connected"));
        assert_eq!(handle.join().unwrap().unwrap(), "CAP-123");
    }

    #[test]
    fn loopback_capture_answers_probes_and_keeps_waiting() {
        let (addr, handle) = spawn_capture("nonce-w", std::time::Duration::from_secs(10));
        let client = insecure_client();
        let probe = client
            .get(format!("https://{addr}/favicon.ico"))
            .send()
            .expect("probe answered");
        assert!(probe.text().unwrap().contains("Waiting for Schwab"));
        let _ = client
            .get(format!("https://{addr}/?code=LATE-9&state=nonce-w"))
            .send()
            .expect("redirect delivered");
        assert_eq!(handle.join().unwrap().unwrap(), "LATE-9");
    }

    #[test]
    fn loopback_capture_aborts_on_a_state_mismatch() {
        let (addr, handle) = spawn_capture("nonce-real", std::time::Duration::from_secs(10));
        let resp = insecure_client()
            .get(format!("https://{addr}/?code=EVIL&state=forged"))
            .send()
            .expect("mismatch still answered");
        assert!(resp.text().unwrap().contains("State mismatch"));
        let err = handle.join().unwrap().unwrap_err();
        assert!(err.to_string().contains("state did not match"), "{err}");
    }

    #[test]
    fn loopback_capture_times_out_when_no_redirect_arrives() {
        let (_addr, handle) = spawn_capture("nonce-t", std::time::Duration::ZERO);
        let err = handle.join().unwrap().unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    /// Live end-to-end smoke of the real three-legged flow; run manually:
    ///
    /// ```text
    /// MARKET_SIGNAL_SCHWAB_CLIENT_ID=… MARKET_SIGNAL_SCHWAB_CLIENT_SECRET=… \
    ///   cargo test --ignored schwab_oauth_live
    /// ```
    ///
    /// Uses an in-memory store so it never touches the real Keychain.
    #[test]
    #[ignore = "interactive: opens a browser and needs a real Schwab login"]
    fn schwab_oauth_live() {
        let client_id = std::env::var("MARKET_SIGNAL_SCHWAB_CLIENT_ID")
            .expect("set MARKET_SIGNAL_SCHWAB_CLIENT_ID");
        let client_secret = std::env::var("MARKET_SIGNAL_SCHWAB_CLIENT_SECRET")
            .expect("set MARKET_SIGNAL_SCHWAB_CLIENT_SECRET");
        let store = Arc::new(InMemoryTokenStore::new());
        store.set(SECRET_CLIENT_SECRET, &client_secret).unwrap();
        let oauth = OauthClient::new(client_id.clone(), store).expect("oauth client");

        let now = Utc::now();
        let code = run_loopback_capture(&client_id, true).expect("captured the redirect code");
        oauth.exchange_code(&code, now).expect("code exchange");
        let token = oauth.valid_access_token(now).expect("a usable access token");
        assert!(!token.is_empty(), "access token should be non-empty");
    }

    #[test]
    fn connection_state_classifies_absent_live_and_lapsed_tokens() {
        let now = Utc::now();
        assert_eq!(connection_state(&None, now), SchwabConnection::NotConnected);
        let live = SchwabTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            access_expires_at: now - Duration::minutes(1), // access expiry is irrelevant here
            refresh_expires_at: now + Duration::days(2),
        };
        assert_eq!(
            connection_state(&Some(live.clone()), now),
            SchwabConnection::Connected
        );
        assert_eq!(
            connection_state(&Some(live), now + Duration::days(3)),
            SchwabConnection::Expired
        );
    }

    #[test]
    fn schwab_status_projects_connection_and_carries_the_refresh_expiry() {
        let now = Utc::now();
        let unconnected = SchwabStatus::build("client-abc".into(), false, None, now);
        assert_eq!(unconnected.client_id, "client-abc");
        assert!(!unconnected.secret_configured);
        assert_eq!(unconnected.connection, SchwabConnection::NotConnected);
        assert_eq!(unconnected.refresh_expires_at, None);

        let refresh_at = now + Duration::days(6);
        let tokens = SchwabTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            access_expires_at: now + Duration::minutes(30),
            refresh_expires_at: refresh_at,
        };
        let connected = SchwabStatus::build("client-abc".into(), true, Some(tokens), now);
        assert!(connected.secret_configured);
        assert_eq!(connected.connection, SchwabConnection::Connected);
        assert_eq!(connected.refresh_expires_at, Some(refresh_at));
    }

    #[test]
    fn schwab_status_reads_an_unreadable_token_set_as_not_connected() {
        let now = Utc::now();
        let store = store_with_secret();
        store.set(SECRET_TOKENS, "{not json").unwrap();
        let status = SchwabStatus::read("client-abc".into(), &*store, now).unwrap();
        assert_eq!(status.client_id, "client-abc");
        assert!(status.secret_configured);
        assert_eq!(status.connection, SchwabConnection::NotConnected);
        assert_eq!(status.refresh_expires_at, None);

        store
            .set_tokens(&SchwabTokens {
                access_token: "a".into(),
                refresh_token: "r".into(),
                access_expires_at: now + Duration::minutes(30),
                refresh_expires_at: now + Duration::days(6),
            })
            .unwrap();
        let status = SchwabStatus::read("client-abc".into(), &*store, now).unwrap();
        assert_eq!(status.connection, SchwabConnection::Connected);

        assert!(SchwabStatus::read("client-abc".into(), &DeniedTokenStore, now).is_err());
    }

    #[test]
    fn schwab_gate_blocks_unless_connected() {
        let ok = schwab_gate(SchwabPresence::Connected);
        assert!(!ok.is_blocked);
        assert!(ok.categories.is_empty());

        for (presence, item) in [
            (SchwabPresence::NotConnected, SCHWAB_NOT_CONNECTED_MSG),
            (SchwabPresence::Unreadable, SCHWAB_UNREADABLE_MSG),
        ] {
            let blocked = schwab_gate(presence);
            assert!(blocked.is_blocked, "{presence:?}");
            assert_eq!(blocked.categories.len(), 1, "{presence:?}");
            let cat = &blocked.categories[0];
            assert_eq!(cat.kind, WarningKind::Schwab);
            assert!(cat.dismiss_id.is_none());
            assert_eq!(cat.items, vec![item.to_string()]);
        }
        assert!(SCHWAB_NOT_CONNECTED_MSG.contains("connect your Schwab account"));
        assert!(SCHWAB_UNREADABLE_MSG.contains("Keychain"));
    }

    #[test]
    fn schwab_presence_tracks_token_presence_and_the_refresh_window() {
        let now = Utc::now();
        let store = store_with_secret();
        assert_eq!(schwab_presence(&*store, now), SchwabPresence::NotConnected);
        store
            .set_tokens(&SchwabTokens {
                access_token: "a".into(),
                refresh_token: "r".into(),
                access_expires_at: now - Duration::minutes(1), // access expired is fine
                refresh_expires_at: now + Duration::days(2),
            })
            .unwrap();
        assert_eq!(schwab_presence(&*store, now), SchwabPresence::Connected);
        assert_eq!(
            schwab_presence(&*store, now + Duration::days(3)),
            SchwabPresence::NotConnected
        );
    }

    #[test]
    fn schwab_presence_reads_a_failed_token_read_as_unreadable() {
        let now = Utc::now();
        assert_eq!(
            schwab_presence(&DeniedTokenStore, now),
            SchwabPresence::Unreadable
        );
        let store = store_with_secret();
        store.set(SECRET_TOKENS, "{not json").unwrap();
        assert_eq!(schwab_presence(&*store, now), SchwabPresence::Unreadable);
        // A double-encoded record: valid JSON, but a string where the record belongs.
        let record = serde_json::to_string(&SchwabTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            access_expires_at: now + Duration::minutes(30),
            refresh_expires_at: now + Duration::days(7),
        })
        .unwrap();
        store
            .set(SECRET_TOKENS, &serde_json::to_string(&record).unwrap())
            .unwrap();
        assert_eq!(schwab_presence(&*store, now), SchwabPresence::Unreadable);
    }
}
