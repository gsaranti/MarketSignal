//! The Schwab bearer-credential rail: the app secret and OAuth tokens. They grant
//! access to the user's brokerage account, so unlike the provider API keys they live
//! in the macOS Keychain (`docs/schwab-integration.md §Token lifecycle`), behind the
//! [`TokenStore`] trait so tests swap in an [`InMemoryTokenStore`].
//!
//! Values written here are never logged and never returned to the webview, and the
//! store is the single home for the tokens: nothing caches a decrypted copy elsewhere.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// The keyring service; the per-secret `name` is the keyring username.
const KEYRING_SERVICE: &str = "market-signal-schwab";

/// The developer-portal app secret, paired with the configured client id.
pub const SECRET_CLIENT_SECRET: &str = "client-secret";

/// The OAuth token set as one JSON [`SchwabTokens`] blob, so the access/refresh pair
/// and their expiries move together atomically.
pub const SECRET_TOKENS: &str = "tokens";

/// The access token lasts 30 minutes and is refreshed transparently; the refresh token
/// lasts 7 days and cannot be extended, so its expiry is set once at the interactive
/// login and carried unchanged across refreshes
/// (`docs/schwab-integration.md §Token lifecycle`).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchwabTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: DateTime<Utc>,
    pub refresh_expires_at: DateTime<Utc>,
}

/// Redacting `Debug`: the expiries print, the token values never do, so no `{:?}` site
/// can leak the bearer credentials. `Serialize` stays: it is the Keychain blob format,
/// written only to the [`TokenStore`] rail.
impl std::fmt::Debug for SchwabTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchwabTokens")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

impl SchwabTokens {
    /// `skew` is a safety margin so a token about to expire mid-request is refreshed
    /// first.
    pub fn access_valid(&self, now: DateTime<Utc>, skew: Duration) -> bool {
        now + skew < self.access_expires_at
    }

    /// Once false, the only recovery is the interactive login; there is no silent
    /// renewal.
    pub fn refresh_valid(&self, now: DateTime<Utc>) -> bool {
        now < self.refresh_expires_at
    }
}

/// Keyed by the `SECRET_*` consts. `Send + Sync` so a single store can be shared into
/// the `spawn_blocking` job closure behind an `Arc`.
pub trait TokenStore: Send + Sync {
    /// `None` when no entry exists.
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, value: &str) -> Result<()>;
    /// A no-op when the entry does not exist.
    fn delete(&self, name: &str) -> Result<()>;

    /// `None` when the account has never been connected; a malformed blob is an error
    /// (a corrupted rail is not an absent one). The error names the parse failure's
    /// kind and position alone, because serde's own message can quote the stored
    /// value.
    fn tokens(&self) -> Result<Option<SchwabTokens>> {
        match self.get(SECRET_TOKENS)? {
            Some(raw) => Ok(Some(serde_json::from_str(&raw).map_err(|e| {
                anyhow!(
                    "parsing stored Schwab tokens: {} at line {} column {}",
                    parse_failure(e.classify()),
                    e.line(),
                    e.column()
                )
            })?)),
            None => Ok(None),
        }
    }

    fn set_tokens(&self, tokens: &SchwabTokens) -> Result<()> {
        let raw = serde_json::to_string(tokens).context("serializing Schwab tokens")?;
        self.set(SECRET_TOKENS, &raw)
    }
}

/// The kind of a token-blob parse failure, in words that never carry the blob.
fn parse_failure(category: serde_json::error::Category) -> &'static str {
    use serde_json::error::Category;
    match category {
        Category::Syntax => "not valid JSON",
        Category::Eof => "truncated",
        Category::Data => "not a token record",
        Category::Io => "unreadable",
    }
}

/// The production store: the macOS Keychain, via `keyring`.
pub struct KeyringTokenStore;

impl KeyringTokenStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(name: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, name)
            .with_context(|| format!("opening Keychain entry {name}"))
    }
}

impl Default for KeyringTokenStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenStore for KeyringTokenStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        match Self::entry(name)?.get_password() {
            Ok(v) => Ok(Some(v)),
            // A missing entry is the ordinary "not connected" state, not an error.
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading Keychain entry {name}")),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        Self::entry(name)?
            .set_password(value)
            .with_context(|| format!("writing Keychain entry {name}"))
    }

    fn delete(&self, name: &str) -> Result<()> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e).with_context(|| format!("deleting Keychain entry {name}")),
        }
    }
}

/// An in-process store for tests: no Keychain, no prompts, no persistence.
#[derive(Default)]
pub struct InMemoryTokenStore {
    map: Mutex<HashMap<String, String>>,
}

impl InMemoryTokenStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TokenStore for InMemoryTokenStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(self.map.lock().expect("token store mutex").get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        self.map
            .lock()
            .expect("token store mutex")
            .insert(name.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.map.lock().expect("token store mutex").remove(name);
        Ok(())
    }
}

/// Every call fails the way a denied Keychain access prompt does (`keyring` maps
/// errSecAuthFailed to `PlatformFailure`), wrapped as [`KeyringTokenStore`] wraps it
/// (`docs/schwab-integration.md §Failure posture`).
#[cfg(test)]
pub struct DeniedTokenStore;

#[cfg(test)]
impl DeniedTokenStore {
    fn denied<T>(op: &str, name: &str) -> Result<T> {
        Err(keyring::Error::PlatformFailure(
            "The user name or passphrase you entered is not correct.".into(),
        ))
        .with_context(|| format!("{op} Keychain entry {name}"))
    }
}

#[cfg(test)]
impl TokenStore for DeniedTokenStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        Self::denied("reading", name)
    }

    fn set(&self, name: &str, _value: &str) -> Result<()> {
        Self::denied("writing", name)
    }

    fn delete(&self, name: &str) -> Result<()> {
        Self::denied("deleting", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tokens(now: DateTime<Utc>) -> SchwabTokens {
        SchwabTokens {
            access_token: "access-abc".to_string(),
            refresh_token: "refresh-xyz".to_string(),
            access_expires_at: now + Duration::minutes(30),
            refresh_expires_at: now + Duration::days(7),
        }
    }

    #[test]
    fn in_memory_store_round_trips_get_set_delete() {
        let store = InMemoryTokenStore::new();
        assert_eq!(store.get(SECRET_CLIENT_SECRET).unwrap(), None);
        store.set(SECRET_CLIENT_SECRET, "sekret").unwrap();
        assert_eq!(
            store.get(SECRET_CLIENT_SECRET).unwrap().as_deref(),
            Some("sekret")
        );
        store.delete(SECRET_CLIENT_SECRET).unwrap();
        assert_eq!(store.get(SECRET_CLIENT_SECRET).unwrap(), None);
        store.delete(SECRET_CLIENT_SECRET).unwrap();
    }

    #[test]
    fn debug_output_redacts_the_token_values() {
        let toks = sample_tokens(Utc::now());
        let dump = format!("{toks:?}");
        assert!(!dump.contains("access-abc"), "{dump}");
        assert!(!dump.contains("refresh-xyz"), "{dump}");
        assert!(dump.contains("access_expires_at"), "{dump}");
        assert!(dump.contains("<redacted>"), "{dump}");
    }

    #[test]
    fn tokens_round_trip_through_the_json_blob() {
        let now = Utc::now();
        let store = InMemoryTokenStore::new();
        assert_eq!(store.tokens().unwrap(), None);
        let toks = sample_tokens(now);
        store.set_tokens(&toks).unwrap();
        assert_eq!(store.tokens().unwrap(), Some(toks));
    }

    #[test]
    fn a_malformed_token_blob_is_an_error_not_a_silent_none() {
        let store = InMemoryTokenStore::new();
        store.set(SECRET_TOKENS, "{not json").unwrap();
        assert!(store.tokens().is_err());
    }

    #[test]
    fn a_malformed_token_blob_error_names_its_kind_and_never_the_stored_value() {
        let store = InMemoryTokenStore::new();
        // A double-encoded record: serde's own message would quote it whole, both
        // tokens included.
        let record = serde_json::to_string(&sample_tokens(Utc::now())).unwrap();
        let double_encoded = serde_json::to_string(&record).unwrap();
        for (blob, kind) in [
            (double_encoded.as_str(), "not a token record"),
            (r#"{"access_token":"access-abc" "refresh_token"}"#, "not valid JSON"),
            (r#"{"access_token":"access-abc","#, "truncated"),
        ] {
            store.set(SECRET_TOKENS, blob).unwrap();
            let chain = format!("{:#}", store.tokens().unwrap_err());
            assert!(!chain.contains("access-abc"), "{chain}");
            assert!(!chain.contains("refresh-xyz"), "{chain}");
            assert!(
                chain.starts_with(&format!("parsing stored Schwab tokens: {kind} at line 1 column ")),
                "{chain}"
            );
        }
    }

    /// Live round-trip against the real macOS Keychain; may prompt. Run manually:
    /// `cargo test --ignored keyring_store_round_trips_live`. Uses a scratch entry name,
    /// never the real `SECRET_*` keys, so it can't clobber a stored credential.
    #[test]
    #[ignore = "touches the real macOS Keychain (may prompt); run manually"]
    fn keyring_store_round_trips_live() {
        let store = KeyringTokenStore::new();
        let key = "test-roundtrip-scratch";
        // Start clean in case a prior aborted run left the entry behind.
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
        store.set(key, "live-value").unwrap();
        assert_eq!(store.get(key).unwrap().as_deref(), Some("live-value"));
        store.delete(key).unwrap();
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
    }

    #[test]
    fn access_validity_honors_the_skew_margin() {
        let now = Utc::now();
        let toks = sample_tokens(now);
        assert!(toks.access_valid(now, Duration::seconds(30)));
        assert!(!toks.access_valid(now + Duration::minutes(30), Duration::seconds(30)));
        assert!(!toks.access_valid(now + Duration::minutes(29), Duration::minutes(2)));
    }

    #[test]
    fn refresh_validity_tracks_the_seven_day_window() {
        let now = Utc::now();
        let toks = sample_tokens(now);
        assert!(toks.refresh_valid(now));
        assert!(toks.refresh_valid(now + Duration::days(6)));
        assert!(!toks.refresh_valid(now + Duration::days(7)));
    }
}
