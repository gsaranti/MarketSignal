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

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

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
/// developer `client_id`, and the exact registered `redirect_uri`. Pure — no I/O — so
/// the query shape is unit-testable.
pub fn authorize_url(client_id: &str) -> String {
    let redirect = encode_component(REDIRECT_URI);
    let client = encode_component(client_id);
    format!("{AUTHORIZE_URL}?response_type=code&client_id={client}&redirect_uri={redirect}")
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

    if open_browser {
        open_url(&authorize_url(client_id));
    }

    // Block for the redirect. A misdirected probe (a browser prefetch, a favicon) that
    // carries no code is answered and ignored; the real redirect resolves the loop.
    loop {
        let request = server.recv().context("awaiting the OAuth redirect")?;
        let target = request.url().to_string();
        match parse_redirect_code(&target) {
            Ok(code) => {
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
    fn authorize_url_carries_code_flow_and_the_encoded_redirect() {
        let url = authorize_url("client-abc@AMER.OAUTHAP");
        assert!(url.starts_with(AUTHORIZE_URL), "{url}");
        assert!(url.contains("response_type=code"), "{url}");
        // The redirect URI is percent-encoded (its ':' and '/' escaped)...
        assert!(url.contains("redirect_uri=https%3A%2F%2F127.0.0.1%3A8182"), "{url}");
        // ...and the client id's reserved characters too.
        assert!(url.contains("client_id=client-abc%40AMER.OAUTHAP"), "{url}");
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
