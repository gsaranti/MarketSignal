//! Schwab three-legged OAuth 2.0 (`docs/schwab-integration.md §Authorization`,
//! §Token lifecycle).
//!
//! The user authenticates in the system browser with their Schwab *brokerage*
//! credentials; Schwab redirects to a self-signed **HTTPS loopback** server the app
//! runs at `https://127.0.0.1:8182`, which captures the one-time authorization code.
//! The code is exchanged for a 30-minute access token and a 7-day refresh token, both
//! parked on the Keychain rail ([`crate::schwab_secrets`]). Thereafter
//! [`OauthClient::valid_access_token`] hands out a live access token, refreshing it
//! transparently — until the 7-day refresh token lapses, at which point it returns
//! [`ReauthRequired`] and the only recovery is another browser login.
//!
//! Token-safety invariant (`docs/schwab-integration.md`): access/refresh tokens are
//! **never logged and never emitted on the progress seam**. The token HTTP calls carry
//! the credentials in an `Authorization` header and a form body that this module builds
//! and drops; only endpoint + status ever reach an error message, never a token or the
//! success body.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{ValidationReport, WarningCategory, WarningKind};
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
/// How long the loopback capture waits for the browser redirect before giving up, so an
/// abandoned login can't park the capture thread (and the run slot) indefinitely.
/// Fully-qualified `std::time::Duration` to avoid shadowing chrono's `Duration`, which
/// the token-lifecycle math uses.
const CAPTURE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// The terminal OAuth state: no stored tokens, or the refresh token has lapsed. The
/// caller must run the interactive browser login again — there is no silent renewal.
/// A distinct type so the run-gate can tell "reconnect Schwab" apart from a transient
/// network error and surface the right prompt.
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

/// Build the browser authorization URL for leg 1: `response_type=code`, the
/// developer `client_id`, the exact registered `redirect_uri`, and a per-run `state`
/// nonce the redirect must echo back (a CSRF guard). Pure — no I/O — so the query shape
/// is unit-testable.
pub fn authorize_url(client_id: &str, state: &str) -> String {
    let redirect = encode_component(REDIRECT_URI);
    let client = encode_component(client_id);
    let state = encode_component(state);
    format!("{AUTHORIZE_URL}?response_type=code&client_id={client}&redirect_uri={redirect}&state={state}")
}

/// Minimal percent-encoding for the query components we emit (`client_id`,
/// `redirect_uri`). Encodes everything outside the RFC-3986 unreserved set, which
/// covers the `:` and `/` in the redirect URI. Kept local so building the authorize
/// URL needs no extra dependency.
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

/// Extract the one-time authorization `code` from the loopback redirect target — the
/// path+query tiny_http hands back, e.g. `/?code=ABC&session=XYZ`. Percent-decoding is
/// handled by the URL parser, so a code with encoded characters round-trips intact.
/// Pure — the live server hands its `request.url()` straight in.
pub fn parse_redirect_code(target: &str) -> Result<String> {
    // A relative target (`/?code=...`) is resolved against a throwaway base so the
    // standard query-pair decoder applies; an absolute redirect URL parses directly.
    let base = reqwest::Url::parse("http://127.0.0.1/").expect("static base URL is valid");
    let parsed = base
        .join(target)
        .with_context(|| format!("parsing OAuth redirect target {target:?}"))?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .filter(|c| !c.is_empty())
        .ok_or_else(|| anyhow!("OAuth redirect carried no authorization code"))
}

/// The `state` nonce echoed on the redirect, if present. The capture compares it to the
/// nonce it issued and rejects a mismatch (a stray or forged redirect).
fn redirect_state(target: &str) -> Option<String> {
    let base = reqwest::Url::parse("http://127.0.0.1/").expect("static base URL is valid");
    let parsed = base.join(target).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
}

/// Schwab's token-endpoint success body (the fields the lifecycle needs). Extra fields
/// (`token_type`, `scope`, `id_token`) are ignored.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    /// Access-token lifetime in seconds (Schwab sends 1800).
    expires_in: i64,
}

/// The OAuth token client: exchanges codes, refreshes access tokens, and answers the
/// connection-state question the run-gate asks. Holds the developer `client_id`, the
/// Keychain-backed [`TokenStore`], and a blocking HTTP client. The `token_url` is a
/// field so tests can point it at a localhost mock ([`crate::test_http`]).
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

    /// The developer app secret from the Keychain rail. Required for the token-endpoint
    /// Basic auth; absent means the connection was never configured.
    fn client_secret(&self) -> Result<String> {
        self.store
            .get(SECRET_CLIENT_SECRET)?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow!("Schwab client secret is not configured"))
    }

    /// POST the token endpoint with Basic auth + a form body, returning the parsed
    /// success response. Single-shot: an authorization code is single-use, so a blind
    /// retry could double-spend it. Never logs the body (it carries the tokens).
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
            // The error body describes the failure (invalid grant, expired code) and
            // carries no token, so it is safe to surface; the success body never is.
            let body = resp.text().unwrap_or_default();
            bail!("Schwab token endpoint returned {status}: {body}");
        }
        resp.json::<TokenResponse>()
            .context("parsing Schwab token response")
    }

    /// Leg 2/3: exchange a captured authorization `code` for the token set and persist
    /// it. The refresh window is anchored at `now` and never extended thereafter.
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

    /// Refresh the access token using the stored refresh token. The 7-day refresh
    /// window is **not** reset — `refresh_expires_at` is carried from the prior set — so
    /// a weekly re-login is unavoidable even under continuous use. Returns
    /// [`ReauthRequired`] when there is nothing to refresh or the window has lapsed.
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
            // Schwab returns a fresh refresh token; adopt it but keep the original
            // expiry, since the 7-day window cannot be extended.
            refresh_token: resp.refresh_token,
            access_expires_at: now + Duration::seconds(resp.expires_in),
            refresh_expires_at: current.refresh_expires_at,
        };
        self.store.set_tokens(&tokens)?;
        Ok(tokens)
    }

    /// A live access token for a Schwab API call: the stored one when still valid,
    /// otherwise a transparent refresh. [`ReauthRequired`] when the account is not
    /// connected or the refresh window has lapsed.
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

    /// Whether a usable connection exists at `now` — tokens present and the refresh
    /// window open. The source-selection gate reads this to decide live-vs-blocked
    /// without attempting a network call.
    pub fn is_connected(&self, now: DateTime<Utc>) -> Result<bool> {
        Ok(self
            .store
            .tokens()?
            .is_some_and(|t| t.refresh_valid(now)))
    }
}

/// The connection state the Settings surface renders, derived from the stored token set
/// without a network call (`docs/interface.md §Connection status` — presence, not a live
/// probe). Three states the UI treats distinctly: never linked, a live connection, and a
/// lapsed 7-day refresh window that forces a fresh browser login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchwabConnection {
    /// No token set stored — the account has never been connected (or was disconnected).
    NotConnected,
    /// Tokens present and the refresh window still open — a usable connection.
    Connected,
    /// Tokens present but the 7-day refresh window has lapsed; only a fresh interactive
    /// login recovers it.
    Expired,
}

/// Classify the stored token set into a [`SchwabConnection`] at `now`. Pure — the command
/// reads the tokens off the Keychain rail and hands them in — so the state machine is
/// unit-testable without a store. Mirrors [`OauthClient::is_connected`]'s
/// refresh-window rule (`Connected` iff `is_connected`), but keeps the `Expired` case
/// distinct so the surface can prompt re-login rather than first-connect.
pub fn connection_state(tokens: &Option<SchwabTokens>, now: DateTime<Utc>) -> SchwabConnection {
    match tokens {
        None => SchwabConnection::NotConnected,
        Some(t) if t.refresh_valid(now) => SchwabConnection::Connected,
        Some(_) => SchwabConnection::Expired,
    }
}

/// What the Settings "Charles Schwab connection" surface renders (`docs/interface.md
/// §Settings`). The `client_id` is a non-secret identifier, returned so the form can
/// pre-fill it; the client *secret* and the tokens never cross this seam — only the
/// `secret_configured` boolean and the derived connection state do, holding the rail's
/// never-return-a-secret invariant (`crate::schwab_secrets`). `refresh_expires_at` drives
/// the weekly-re-login heads-up.
#[derive(Debug, Clone, Serialize)]
pub struct SchwabStatus {
    pub client_id: String,
    pub secret_configured: bool,
    pub connection: SchwabConnection,
    pub refresh_expires_at: Option<DateTime<Utc>>,
}

impl SchwabStatus {
    /// Assemble the status from the pieces the command reads off storage: the configured
    /// `client_id`, whether the client secret is present on the Keychain rail, and the
    /// stored token set (or `None`). Pure so the projection is unit-testable.
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
}

/// The message a local job returns when it is run without a connected Schwab account
/// (`docs/schwab-integration.md §A connected Schwab account is required`). It is the
/// [`schwab_gate`] category's item, so the run-gate block and the (future) warning band
/// speak with one voice. Job-neutral ("this analysis") because the gate is shared by both
/// local jobs — Portfolio Analysis today, Trade Opportunities once it lands.
pub const SCHWAB_NOT_CONNECTED_MSG: &str =
    "Schwab account not connected — connect your Schwab account (weekly re-login) to run this analysis.";

/// The local-suite Schwab-connection gate, as a [`ValidationReport`] carrying one
/// [`WarningKind::Schwab`] category when disconnected — the exact shape
/// `local_model::local_gate` produces for the daemon. Pure over the connection bool, so
/// it is both unit-testable and the single producer the run-gate and the (deferred)
/// local-suite warning band share. Independent of the cloud-report gate
/// (`config::validate`): a disconnected Schwab account blocks only the local jobs, never
/// the Market Signal Report. `connected` comes from [`OauthClient::is_connected`].
pub fn schwab_gate(connected: bool) -> ValidationReport {
    let mut categories = Vec::new();
    if !connected {
        categories.push(WarningCategory {
            kind: WarningKind::Schwab,
            title: "Charles Schwab connection".to_string(),
            items: vec![SCHWAB_NOT_CONNECTED_MSG.to_string()],
            dismiss_id: None,
        });
    }
    let is_blocked = categories.iter().any(|c| c.kind.is_blocking());
    ValidationReport {
        categories,
        is_blocked,
    }
}

/// Run the one-shot loopback capture: stand up the self-signed HTTPS server on
/// `127.0.0.1:8182`, open the browser at the authorize URL, block for the single
/// redirect, and return its authorization code. Live-only — it needs a real browser
/// login — so it is exercised by `schwab_connect` and an `#[ignore]` smoke, never the
/// offline suite. `open_browser` is false in the smoke (which drives the redirect by
/// hand) and true in the command.
pub fn run_loopback_capture(client_id: &str, open_browser: bool) -> Result<String> {
    // A self-signed cert for 127.0.0.1: no CA signs a loopback address, so the browser
    // shows a one-time warning the user clicks through (`docs/schwab-integration.md`).
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])
        .context("generating self-signed loopback certificate")?;
    let ssl = tiny_http::SslConfig {
        certificate: certified.cert.pem().into_bytes(),
        private_key: certified.signing_key.serialize_pem().into_bytes(),
    };
    let server = tiny_http::Server::https(LOOPBACK_ADDR, ssl)
        .map_err(|e| anyhow!("binding loopback OAuth server on {LOOPBACK_ADDR}: {e}"))?;

    // A per-run nonce round-tripped through `state`: a redirect that doesn't echo the
    // exact value this capture issued is not our login, and is rejected.
    let state = uuid::Uuid::new_v4().to_string();
    if open_browser {
        open_url(&authorize_url(client_id, &state));
    }

    // Wait for the redirect, bounded by CAPTURE_TIMEOUT so an abandoned browser login
    // can't park this thread (and the run slot) forever. A misdirected probe (a browser
    // prefetch, a favicon) that carries no code is answered and ignored; the real
    // redirect resolves the loop.
    let deadline = Instant::now() + CAPTURE_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("timed out waiting for the Schwab authorization redirect");
        }
        let Some(request) = server
            .recv_timeout(remaining)
            .context("awaiting the OAuth redirect")?
        else {
            bail!("timed out waiting for the Schwab authorization redirect");
        };
        let target = request.url().to_string();
        match parse_redirect_code(&target) {
            Ok(code) => {
                // The code-bearing request must carry our exact state, or it is a forged
                // or stale redirect, not the login we initiated.
                if redirect_state(&target).as_deref() != Some(state.as_str()) {
                    let _ = request
                        .respond(tiny_http::Response::from_string("State mismatch — ignoring."));
                    bail!("OAuth redirect state did not match the issued nonce; aborting");
                }
                let _ = request.respond(tiny_http::Response::from_string(CLOSE_TAB_HTML));
                return Ok(code);
            }
            Err(_) => {
                let _ = request.respond(tiny_http::Response::from_string("Waiting for Schwab…"));
            }
        }
    }
}

/// The page shown in the browser once the code is captured.
const CLOSE_TAB_HTML: &str =
    "<html><body><h2>Schwab connected.</h2><p>You can close this tab and return to Market Signal.</p></body></html>";

/// Open a URL in the system browser (macOS `open`). Best-effort — a failure just means
/// the user opens the printed URL themselves; the capture server is already listening.
fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schwab_secrets::InMemoryTokenStore;
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
        // The redirect URI is percent-encoded (its ':' and '/' escaped)...
        assert!(url.contains("redirect_uri=https%3A%2F%2F127.0.0.1%3A8182"), "{url}");
        // ...and the client id's reserved characters too.
        assert!(url.contains("client_id=client-abc%40AMER.OAUTHAP"), "{url}");
        // ...and the CSRF state nonce is carried.
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
        // Percent-encoded characters in the code are decoded.
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
        // The token set is persisted for later refresh.
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
        // No mock server needed: a valid access token is returned without a network call.
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
        // The refresh window is carried, not reset: still the original 3-day expiry.
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

    /// Live end-to-end smoke: the real three-legged flow against Schwab. Ignored by
    /// default (it opens a browser and needs an interactive brokerage login); run
    /// manually with the developer credentials in the environment:
    ///
    /// ```text
    /// MARKET_SIGNAL_SCHWAB_CLIENT_ID=… MARKET_SIGNAL_SCHWAB_CLIENT_SECRET=… \
    ///   cargo test --ignored schwab_oauth_live
    /// ```
    ///
    /// It stands up the loopback server, opens the login, captures the redirect, and
    /// exchanges the code — asserting a usable access token comes back. Uses an
    /// in-memory store so it never touches the real Keychain.
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
        // Same tokens, evaluated past the refresh window → Expired, not NotConnected.
        assert_eq!(
            connection_state(&Some(live), now + Duration::days(3)),
            SchwabConnection::Expired
        );
    }

    #[test]
    fn schwab_status_projects_connection_and_carries_the_refresh_expiry() {
        let now = Utc::now();
        // Never connected: no tokens, so no refresh expiry, and the secret flag/id pass
        // straight through.
        let unconnected = SchwabStatus::build("client-abc".into(), false, None, now);
        assert_eq!(unconnected.client_id, "client-abc");
        assert!(!unconnected.secret_configured);
        assert_eq!(unconnected.connection, SchwabConnection::NotConnected);
        assert_eq!(unconnected.refresh_expires_at, None);

        // Connected: the refresh expiry is surfaced for the weekly-re-login heads-up.
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
    fn schwab_gate_blocks_only_when_disconnected() {
        // Connected → no category, not blocked (the local job proceeds).
        let ok = schwab_gate(true);
        assert!(!ok.is_blocked);
        assert!(ok.categories.is_empty());

        // Disconnected → one blocking WarningKind::Schwab category carrying the reconnect
        // prompt. This is the producer both the run-gate block and the (deferred)
        // local-suite warning band consume — parity with `local_gate`/LocalModels.
        let blocked = schwab_gate(false);
        assert!(blocked.is_blocked);
        assert_eq!(blocked.categories.len(), 1);
        let cat = &blocked.categories[0];
        assert_eq!(cat.kind, WarningKind::Schwab);
        assert!(cat.dismiss_id.is_none());
        assert!(
            cat.items[0].contains("connect your Schwab account"),
            "{:?}",
            cat.items
        );
    }

    #[test]
    fn is_connected_tracks_token_presence_and_the_refresh_window() {
        let now = Utc::now();
        let store = store_with_secret();
        let oauth = client(store.clone(), "http://127.0.0.1:1/unused");
        assert!(!oauth.is_connected(now).unwrap()); // no tokens yet
        store
            .set_tokens(&SchwabTokens {
                access_token: "a".into(),
                refresh_token: "r".into(),
                access_expires_at: now - Duration::minutes(1), // access expired is fine
                refresh_expires_at: now + Duration::days(2),
            })
            .unwrap();
        assert!(oauth.is_connected(now).unwrap()); // refresh window open → connected
        assert!(!oauth.is_connected(now + Duration::days(3)).unwrap()); // lapsed
    }
}
