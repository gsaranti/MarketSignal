//! The Schwab bearer-credential rail: the app secret and OAuth tokens.
//!
//! These are the only credentials in the app that grant access to the user's
//! *brokerage account*, so — unlike the less-sensitive provider API keys, which
//! live in the SQLite `app_settings` store — they are held in the **macOS
//! Keychain** (`docs/schwab-integration.md §Token lifecycle`). Access goes through
//! the [`TokenStore`] trait so the OAuth flow (`schwab_oauth`) and the live source
//! (`schwab_live`) never bind to Keychain directly: the real [`KeyringTokenStore`]
//! is swapped for an [`InMemoryTokenStore`] in tests, mirroring the trait-stub
//! discipline the rest of the pipeline uses (`HoldingsSource`, `Embedder`).
//!
//! Two invariants hold across the rail: values written here are **never logged and
//! never returned to the webview** (the Settings view collapses Schwab to a
//! connected boolean, exactly as it does for the API keys), and the store is the
//! single home for the tokens — the source-selection gate reads connection state
//! from it, nothing caches a decrypted copy elsewhere.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Keyring *service* the Schwab entries group under. Fixed, app-scoped — the
/// per-secret `name` (below) is the keyring *username*.
const KEYRING_SERVICE: &str = "market-signal-schwab";

/// The app (developer-portal) secret, paired with the configured client id. Held
/// here rather than in `app_settings` because it is a bearer credential
/// (`docs/schwab-integration.md §Token lifecycle`).
pub const SECRET_CLIENT_SECRET: &str = "client-secret";

/// The current OAuth token set, stored as a JSON [`SchwabTokens`] blob under one
/// entry so the access/refresh pair and their expiries move together atomically.
pub const SECRET_TOKENS: &str = "tokens";

/// The OAuth token set and its lifecycle bounds (`docs/schwab-integration.md
/// §Token lifecycle`). The access token lasts 30 minutes and is refreshed
/// transparently; the refresh token lasts 7 days and **cannot be extended**, so its
/// expiry is set once at the interactive login and carried unchanged across
/// refreshes — lapse forces a fresh browser re-login.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchwabTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: DateTime<Utc>,
    pub refresh_expires_at: DateTime<Utc>,
}

/// Manual, redacting `Debug`: the expiries print (they drive the lifecycle decisions a
/// debug dump exists to explain), the token values never do — a derived `Debug` would
/// hand any future `{:?}` site the raw bearer credentials. Same discipline as
/// `config::MainAgentConfig`'s deliberate no-`Debug` bound. (`Serialize` stays: it is
/// the Keychain blob format, written only to the [`TokenStore`] rail.)
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
    /// Whether the access token is still usable at `now`, keeping a `skew` safety
    /// margin so a token about to expire mid-request is refreshed first.
    pub fn access_valid(&self, now: DateTime<Utc>, skew: Duration) -> bool {
        now + skew < self.access_expires_at
    }

    /// Whether the refresh token is still usable at `now`. Once this is false the
    /// only recovery is the interactive login — there is no silent renewal.
    pub fn refresh_valid(&self, now: DateTime<Utc>) -> bool {
        now < self.refresh_expires_at
    }
}

/// The Schwab credential store: get / set / delete a secret by its fixed `name`
/// (one of the `SECRET_*` consts). `Send + Sync` so a single store can be shared
/// into the `spawn_blocking` job closure behind an `Arc`.
pub trait TokenStore: Send + Sync {
    /// The stored value for `name`, or `None` when no entry exists.
    fn get(&self, name: &str) -> Result<Option<String>>;
    /// Write (creating or replacing) the value for `name`.
    fn set(&self, name: &str, value: &str) -> Result<()>;
    /// Remove the entry for `name`; a no-op when it does not exist.
    fn delete(&self, name: &str) -> Result<()>;

    /// Read and deserialize the stored [`SchwabTokens`], or `None` when the account
    /// has never been connected. A malformed blob is an error (a corrupted rail is
    /// not the same as an absent one).
    fn tokens(&self) -> Result<Option<SchwabTokens>> {
        match self.get(SECRET_TOKENS)? {
            Some(raw) => Ok(Some(
                serde_json::from_str(&raw).context("parsing stored Schwab tokens")?,
            )),
            None => Ok(None),
        }
    }

    /// Serialize and persist the token set (replacing any prior one).
    fn set_tokens(&self, tokens: &SchwabTokens) -> Result<()> {
        let raw = serde_json::to_string(tokens).context("serializing Schwab tokens")?;
        self.set(SECRET_TOKENS, &raw)
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

/// An in-process store for tests: no Keychain, no prompts, no persistence beyond
/// the process. Exercises every code path that reads or writes the rail without a
/// live credential store.
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
        // Deleting an absent entry is a no-op, not an error.
        store.delete(SECRET_CLIENT_SECRET).unwrap();
    }

    #[test]
    fn debug_output_redacts_the_token_values() {
        let toks = sample_tokens(Utc::now());
        let dump = format!("{toks:?}");
        assert!(!dump.contains("access-abc"), "{dump}");
        assert!(!dump.contains("refresh-xyz"), "{dump}");
        // The lifecycle fields still print — the redaction is targeted, not a blanket.
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

    /// Live round-trip against the real macOS Keychain (the production rail). Ignored
    /// by default — it writes to and cleans up a throwaway entry under the Schwab
    /// service and may raise a Keychain access prompt on first run. Run manually:
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
        // Deleting an absent entry is a no-op, and the value is gone.
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
    }

    #[test]
    fn access_validity_honors_the_skew_margin() {
        let now = Utc::now();
        let toks = sample_tokens(now);
        // Valid now with a small skew...
        assert!(toks.access_valid(now, Duration::seconds(30)));
        // ...but not once `now + skew` reaches the 30-minute expiry.
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
