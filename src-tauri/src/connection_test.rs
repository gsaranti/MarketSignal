//! "Test connection" — validate one configured provider credential with a single
//! live authenticated request, without spending model tokens
//! (`docs/configuration.md` Settings).
//!
//! Mirrors the BUILD.md spine: the provider HTTP call is an app-layer adapter
//! detail. The blocking `reqwest::blocking` request is offloaded via
//! `spawn_blocking` at the Tauri command seam (`lib.rs`), the same way
//! `generate_report_manual` keeps `reqwest::blocking` off the async runtime
//! thread. This is the first code that talks to FMP and Tavily; the model
//! adapter (`model_agent`) already covers OpenAI/Anthropic, and the Anthropic
//! version header is reused from there.
//!
//! Each provider's request function is split from a pure `interpret_*` so the
//! pass/fail logic — notably FMP, which can report an error either in the status
//! or in a 200 body — is unit-testable offline. (Verified live Jun 2026: an
//! invalid/missing FMP key returns 401 with an `{"Error Message": ...}` body on
//! both `/stable/` and legacy `/api/v3/`; the 200-with-error-body path is kept as
//! defensive cover for FMP's other conditions, e.g. plan/rate-limit, that do.)
//! The request validates the credential only; it never spends model tokens and
//! does not change the execution gate (which checks credential *presence*, not
//! validity — see `config::validate`).

use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;

/// A lightweight, no-/low-cost authenticated endpoint per provider — enough to
/// confirm the key is accepted. OpenAI/Anthropic `/v1/models` and Tavily
/// `/usage` are metadata endpoints (no token/credit spend); FMP `quote` is a
/// free-tier call that counts against the daily request allowance.
const OPENAI_MODELS_URL: &str = "https://api.openai.com/v1/models";
const ANTHROPIC_MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const FMP_QUOTE_URL: &str = "https://financialmodelingprep.com/stable/quote";
const FRED_OBSERVATIONS_URL: &str = "https://api.stlouisfed.org/fred/series/observations";
const TAVILY_USAGE_URL: &str = "https://api.tavily.com/usage";

/// Short timeout: a health check should fail fast, not park for the model
/// adapter's 120s ceiling.
const TEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Which credential to test. Distinct from `model_agent::Provider` (which only
/// models the two *model* providers): test-connection covers all four
/// user-supplied credentials, including the two data providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialProvider {
    OpenAi,
    Anthropic,
    Fmp,
    Fred,
    Tavily,
}

impl CredentialProvider {
    /// Parse the frontend-facing label (matches the `CredentialKey` union and the
    /// `app_settings` credential keys).
    pub fn from_label(label: &str) -> Result<Self> {
        match label {
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            "fmp" => Ok(Self::Fmp),
            "fred" => Ok(Self::Fred),
            "tavily" => Ok(Self::Tavily),
            other => bail!("unknown credential provider {other:?}"),
        }
    }

    /// Human-readable provider name for the result message.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Fmp => "Financial Modeling Prep",
            Self::Fred => "FRED",
            Self::Tavily => "Tavily",
        }
    }
}

/// The outcome of one test, surfaced per-credential in Settings. Never carries
/// the secret — only whether the request was accepted and a short message.
/// `camelCase` is explicit so the wire contract with the TS `ConnectionTestResult`
/// holds even if a multi-word field is added later.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub detail: String,
}

impl ConnectionTestResult {
    fn ok() -> Self {
        Self {
            ok: true,
            detail: "Connected — the key is valid.".to_string(),
        }
    }

    fn fail(detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            detail: detail.into(),
        }
    }

    /// No saved value to test — the command short-circuits before any network
    /// call and returns this.
    pub fn not_configured() -> Self {
        Self::fail("Not configured — save a value first, then test.")
    }
}

fn build_client() -> Result<Client> {
    Client::builder()
        .timeout(TEST_TIMEOUT)
        .build()
        .context("building the connection-test HTTP client")
}

/// Run the test for one provider against `api_key`. Builds its own blocking
/// client, so the caller offloads this whole function through `spawn_blocking`.
pub fn run_test(provider: CredentialProvider, api_key: &str) -> ConnectionTestResult {
    let http = match build_client() {
        Ok(c) => c,
        Err(e) => return ConnectionTestResult::fail(format!("Couldn't start the connection test: {e}")),
    };
    match provider {
        CredentialProvider::OpenAi => {
            let sent = http.get(OPENAI_MODELS_URL).bearer_auth(api_key).send();
            from_status_only(provider, sent.map(|r| r.status().as_u16()))
        }
        CredentialProvider::Anthropic => {
            let sent = http
                .get(ANTHROPIC_MODELS_URL)
                .header("x-api-key", api_key)
                .header("anthropic-version", crate::model_agent::ANTHROPIC_VERSION)
                .send();
            from_status_only(provider, sent.map(|r| r.status().as_u16()))
        }
        CredentialProvider::Tavily => {
            let sent = http.get(TAVILY_USAGE_URL).bearer_auth(api_key).send();
            match sent {
                Ok(r) => interpret_tavily(r.status().as_u16()),
                Err(e) => network_failure(provider, &e),
            }
        }
        CredentialProvider::Fmp => {
            // FMP takes the key as a query param (never an Authorization header).
            // A wrong key returns 401 (verified live), but FMP can also carry an
            // error in a 200 body, so the body is read and handed to the interpreter.
            let sent = http
                .get(FMP_QUOTE_URL)
                .query(&[("symbol", "AAPL"), ("apikey", api_key)])
                .send();
            match sent {
                Ok(r) => {
                    let status = r.status().as_u16();
                    let body = r.text().unwrap_or_default();
                    interpret_fmp(status, &body)
                }
                Err(e) => network_failure(provider, &e),
            }
        }
        CredentialProvider::Fred => {
            // FRED takes the key as the `api_key` query param; a bad key returns
            // HTTP 400 with a JSON `error_message` that names api_key (read by
            // interpret_fred). A known-good series keeps a 400 attributable to the
            // key, not a bad series_id.
            let sent = http
                .get(FRED_OBSERVATIONS_URL)
                .query(&[
                    ("series_id", "DGS10"),
                    ("api_key", api_key),
                    ("file_type", "json"),
                    ("limit", "1"),
                ])
                .send();
            match sent {
                Ok(r) => {
                    let status = r.status().as_u16();
                    let body = r.text().unwrap_or_default();
                    interpret_fred(status, &body)
                }
                Err(e) => network_failure(provider, &e),
            }
        }
    }
}

fn network_failure(provider: CredentialProvider, err: &reqwest::Error) -> ConnectionTestResult {
    ConnectionTestResult::fail(format!("Couldn't reach {}: {err}", provider.display_name()))
}

/// Map a status-only result (OpenAI / Anthropic), including a transport error.
fn from_status_only(
    provider: CredentialProvider,
    status: std::result::Result<u16, reqwest::Error>,
) -> ConnectionTestResult {
    match status {
        Ok(code) => interpret_status_only(provider, code),
        Err(e) => network_failure(provider, &e),
    }
}

/// For OpenAI/Anthropic the HTTP status alone is decisive: 2xx means the key was
/// accepted; 401/403 means it was rejected; anything else is unexpected.
fn interpret_status_only(provider: CredentialProvider, status: u16) -> ConnectionTestResult {
    let name = provider.display_name();
    if (200..300).contains(&status) {
        ConnectionTestResult::ok()
    } else if status == 401 || status == 403 {
        ConnectionTestResult::fail(format!("{name} rejected the key (HTTP {status})."))
    } else {
        ConnectionTestResult::fail(format!("{name} returned an unexpected response (HTTP {status})."))
    }
}

/// FMP needs dual detection. A rejected key surfaces as a non-2xx status — live,
/// an invalid/missing key is a 401 (handled first, before the body is parsed) —
/// but FMP can also report an error as a 200 whose JSON body is an
/// `{"Error Message": ...}` object, so a 200 body is inspected too. A 200 array
/// (including an empty `[]`, which FMP returns for "no data") means the key was
/// accepted.
fn interpret_fmp(status: u16, body: &str) -> ConnectionTestResult {
    if status == 401 || status == 403 {
        return ConnectionTestResult::fail(format!(
            "Financial Modeling Prep rejected the key (HTTP {status})."
        ));
    }
    if !(200..300).contains(&status) {
        return ConnectionTestResult::fail(format!(
            "Financial Modeling Prep returned an unexpected response (HTTP {status})."
        ));
    }
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(body) {
        if map.contains_key("Error Message") {
            return ConnectionTestResult::fail("Financial Modeling Prep rejected the key.");
        }
    }
    ConnectionTestResult::ok()
}

/// FRED's observations endpoint returns 200 for a valid key. A bad / missing key
/// is an HTTP 400 whose JSON `error_message` names `api_key` — distinct from FMP's
/// 401, and the reason this provider needs its own interpreter. A 400 whose message
/// is not an api_key problem (e.g. a bad series_id, which the fixed known-good
/// series should preclude) is reported as unexpected rather than a key rejection.
fn interpret_fred(status: u16, body: &str) -> ConnectionTestResult {
    if (200..300).contains(&status) {
        return ConnectionTestResult::ok();
    }
    if status == 400 {
        let msg = serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|v| {
                v.get("error_message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if msg.to_ascii_lowercase().contains("api_key") {
            return ConnectionTestResult::fail("FRED rejected the key (HTTP 400).");
        }
    }
    ConnectionTestResult::fail(format!("FRED returned an unexpected response (HTTP {status})."))
}

/// Tavily's `/usage` returns 200 for a valid key and 401 for a bad one. A 404 is
/// distinct — the endpoint may be unavailable on the key's plan — so it is
/// reported separately rather than as an auth failure.
fn interpret_tavily(status: u16) -> ConnectionTestResult {
    if (200..300).contains(&status) {
        ConnectionTestResult::ok()
    } else if status == 401 || status == 403 {
        ConnectionTestResult::fail(format!("Tavily rejected the key (HTTP {status})."))
    } else if status == 404 {
        ConnectionTestResult::fail(
            "Tavily's usage endpoint is unavailable on this plan — the key couldn't be checked here.",
        )
    } else {
        ConnectionTestResult::fail(format!("Tavily returned an unexpected response (HTTP {status})."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_label_round_trips_and_rejects_unknown() {
        for (label, want) in [
            ("openai", CredentialProvider::OpenAi),
            ("anthropic", CredentialProvider::Anthropic),
            ("fmp", CredentialProvider::Fmp),
            ("fred", CredentialProvider::Fred),
            ("tavily", CredentialProvider::Tavily),
        ] {
            assert_eq!(CredentialProvider::from_label(label).unwrap(), want);
        }
        assert!(CredentialProvider::from_label("bogus").is_err());
    }

    #[test]
    fn status_only_ok_auth_and_generic() {
        let p = CredentialProvider::OpenAi;
        assert!(interpret_status_only(p, 200).ok);
        let auth = interpret_status_only(p, 401);
        assert!(!auth.ok);
        assert!(auth.detail.contains("rejected"), "{}", auth.detail);
        let other = interpret_status_only(p, 500);
        assert!(!other.ok);
        assert!(other.detail.contains("500"), "{}", other.detail);
        // 403 is treated as a rejection too.
        assert!(!interpret_status_only(CredentialProvider::Anthropic, 403).ok);
    }

    #[test]
    fn tavily_404_is_distinct_from_auth_failure() {
        let auth = interpret_tavily(401);
        assert!(!auth.ok && auth.detail.contains("rejected"), "{}", auth.detail);
        let missing = interpret_tavily(404);
        assert!(!missing.ok);
        assert!(missing.detail.contains("usage endpoint"), "{}", missing.detail);
        assert!(interpret_tavily(200).ok);
    }

    #[test]
    fn fmp_200_array_is_success() {
        let res = interpret_fmp(200, r#"[{"symbol":"AAPL","price":201.5}]"#);
        assert!(res.ok, "{}", res.detail);
    }

    #[test]
    fn fmp_empty_array_is_success() {
        // FMP returns [] for "no data found", which is a valid key, not an error.
        assert!(interpret_fmp(200, "[]").ok);
    }

    #[test]
    fn fmp_200_with_error_message_is_auth_failure() {
        // The case a status-only check would miss: HTTP 200, error in the body.
        let body = r#"{"Error Message":"Invalid API KEY. Please retry or visit our documentation"}"#;
        let res = interpret_fmp(200, body);
        assert!(!res.ok);
        assert!(res.detail.contains("rejected"), "{}", res.detail);
    }

    #[test]
    fn fmp_401_is_a_failure() {
        let res = interpret_fmp(401, "");
        assert!(!res.ok);
        assert!(res.detail.contains("401"), "{}", res.detail);
    }

    #[test]
    fn fmp_401_with_error_body_is_a_failure() {
        // Verified live (Jun 2026): an invalid/missing key returns 401 *with* an
        // `{"Error Message": ...}` body on /stable/ and /api/v3/. The status check
        // must win and report the 401, short-circuiting before the body is parsed.
        let body = r#"{"Error Message":"Invalid API KEY. Feel free to create a Free API Key..."}"#;
        let res = interpret_fmp(401, body);
        assert!(!res.ok);
        assert!(res.detail.contains("401"), "{}", res.detail);
    }

    #[test]
    fn fmp_non_2xx_is_a_failure() {
        assert!(!interpret_fmp(500, "").ok);
    }

    #[test]
    fn fred_200_is_success() {
        let body = r#"{"observations":[{"date":"2026-06-04","value":"4.30"}]}"#;
        assert!(interpret_fred(200, body).ok);
    }

    #[test]
    fn fred_400_with_api_key_message_is_a_rejection() {
        // FRED's bad-key signal: HTTP 400 with an error_message that names api_key.
        let body = r#"{"error_code":400,"error_message":"Bad Request. The value for variable api_key is not registered, is not active, or is otherwise invalid."}"#;
        let res = interpret_fred(400, body);
        assert!(!res.ok);
        assert!(res.detail.contains("rejected"), "{}", res.detail);
    }

    #[test]
    fn fred_other_400_and_5xx_are_unexpected_not_rejections() {
        // A 400 without an api_key message (e.g. a bad series_id) is not a key
        // rejection, and a 5xx is a provider-side problem — both report "unexpected".
        let other_400 = interpret_fred(400, r#"{"error_message":"Bad Request. Something else."}"#);
        assert!(!other_400.ok);
        assert!(other_400.detail.contains("unexpected"), "{}", other_400.detail);
        assert!(interpret_fred(500, "").detail.contains("unexpected"));
    }

    #[test]
    fn not_configured_is_a_failure_result() {
        let res = ConnectionTestResult::not_configured();
        assert!(!res.ok);
        assert!(res.detail.contains("Not configured"), "{}", res.detail);
    }
}
