//! The Settings store: persist user configuration — agent models, API tokens,
//! provider credentials, and the local-suite model config — into `app_settings`,
//! and project it back for the Settings view (`docs/configuration.md`,
//! `docs/interface.md §Settings`).
//!
//! Two halves, mirroring `research`: the pure logic lives here; the thin
//! `#[tauri::command]` wrappers live in `lib.rs`. One load-bearing rule: a secret
//! never round-trips to the webview. `view_from_config` collapses each credential
//! to a `configured` boolean and never carries the stored key. Model slugs and
//! the local-suite values (endpoint, roster ids) are not secret, so they are
//! returned for the form to pre-fill.
//!
//! Three independent submissions (`docs/configuration.md §API Tokens` — the
//! two-token gate is scoped to the cloud submission, never to Settings as a
//! whole): `save` (agent models + API tokens, token-gated),
//! `save_provider_credentials` (FMP / FRED / Tavily, ungated), and
//! `save_local_models` (daemon endpoint + roster, ungated) — so a cloud-keyless
//! machine can complete local-suite setup.

use anyhow::{bail, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::config::{
    AppConfig, KEY_ANTHROPIC_API_KEY, KEY_BALANCED_AGENT_MODEL, KEY_BEAR_AGENT_MODEL,
    KEY_BULL_AGENT_MODEL, KEY_FMP_API_KEY, KEY_FRED_API_KEY, KEY_LOCAL_DAEMON_ENDPOINT,
    KEY_LOCAL_EMBEDDER_MODEL, KEY_LOCAL_FAST_MODEL, KEY_LOCAL_REASONER_MODEL,
    KEY_MAIN_AGENT_MODEL, KEY_OPENAI_API_KEY, KEY_TAVILY_API_KEY,
};
use crate::model_agent::AgentModel;
use crate::storage;
use crate::vector_memory;

/// One selectable model for the Settings dropdown: the slug persisted in
/// `app_settings`, a display name, and the provider it groups under.
#[derive(Debug, Clone, Serialize)]
pub struct ModelOption {
    pub slug: String,
    pub label: String,
    pub provider: String,
}

/// The four agent slots' current selections — each the persisted slug, or "" when
/// unset. The form pre-selects these and submits them back, so a saved (or
/// env-sourced) selection round-trips without loss.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentModels {
    pub main: String,
    pub bull: String,
    pub bear: String,
    pub balanced: String,
}

/// Whether each credential is configured — never the value itself. The form shows
/// "saved" vs "not set" from this, so no secret leaves the backend.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CredentialStatus {
    pub openai: bool,
    pub anthropic: bool,
    pub fmp: bool,
    pub fred: bool,
    pub tavily: bool,
}

/// The local-analysis-models values (`docs/configuration.md §Local Models`):
/// the Ollama daemon endpoint plus the roster ids — reasoner and embedder
/// required for the presence gate, the fast tier optional. None of these is a
/// secret, so unlike credentials they round-trip in full: the view carries the
/// stored values ("" when unset) and the save submits all four verbatim.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalModelSettings {
    pub daemon_endpoint: String,
    pub reasoner_model: String,
    pub fast_model: String,
    pub embedder_model: String,
}

/// What the Settings view renders: the current model selections, the
/// per-credential configured flags, the local-suite model config, and the model
/// dropdown's options.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub models: AgentModels,
    pub credentials: CredentialStatus,
    pub local_models: LocalModelSettings,
    pub available_models: Vec<ModelOption>,
}

/// The API-token half of the cloud save request. Each field is `Some` only when
/// the user entered a new value; `None` (or a blank string) means "leave the
/// stored value unchanged" — so a re-save never wipes a key the form never
/// re-displays. The FMP / FRED / Tavily provider credentials are deliberately
/// not here — they save through [`ProviderCredentialUpdate`], outside the token
/// gate (`docs/configuration.md §API Tokens`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CredentialUpdate {
    pub openai: Option<String>,
    pub anthropic: Option<String>,
}

/// The external data-provider credentials' own save request — independent of the
/// token-gated cloud submission, so a cloud-keyless machine persists FMP / FRED
/// for the local suite (`docs/configuration.md §External Data Provider
/// Credentials`). Same update semantics as [`CredentialUpdate`]: `None` / blank
/// leaves the stored secret unchanged.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProviderCredentialUpdate {
    pub fmp: Option<String>,
    pub fred: Option<String>,
    pub tavily: Option<String>,
}

/// The selectable-model option list, sourced from `AgentModel` so slugs and
/// display names have a single backend home — the frontend never re-encodes the
/// pairing.
pub fn available_models() -> Vec<ModelOption> {
    AgentModel::ALL
        .iter()
        .map(|m| ModelOption {
            slug: m.config_label().to_string(),
            label: m.display_name().to_string(),
            provider: m.provider().display_name().to_string(),
        })
        .collect()
}

/// A value that is set and non-blank.
fn is_configured(opt: &Option<String>) -> bool {
    opt.as_deref().map(str::trim).is_some_and(|s| !s.is_empty())
}

/// Project an `AppConfig` into the Settings view. Pure over the config — no env,
/// no DB — so the masking and model projection are unit-testable.
pub fn view_from_config(cfg: &AppConfig) -> SettingsView {
    SettingsView {
        models: AgentModels {
            main: cfg.main_agent_model.clone().unwrap_or_default(),
            bull: cfg.bull_agent_model.clone().unwrap_or_default(),
            bear: cfg.bear_agent_model.clone().unwrap_or_default(),
            balanced: cfg.balanced_agent_model.clone().unwrap_or_default(),
        },
        credentials: CredentialStatus {
            openai: is_configured(&cfg.openai_api_key),
            anthropic: is_configured(&cfg.anthropic_api_key),
            fmp: is_configured(&cfg.fmp_api_key),
            fred: is_configured(&cfg.fred_api_key),
            tavily: is_configured(&cfg.tavily_api_key),
        },
        local_models: LocalModelSettings {
            daemon_endpoint: cfg.local_daemon_endpoint.clone().unwrap_or_default(),
            reasoner_model: cfg.local_reasoner_model.clone().unwrap_or_default(),
            fast_model: cfg.local_fast_model.clone().unwrap_or_default(),
            embedder_model: cfg.local_embedder_model.clone().unwrap_or_default(),
        },
        available_models: available_models(),
    }
}

/// Load the Settings view from the persisted store (env fallback per field).
pub fn load_view(conn: &Connection) -> SettingsView {
    view_from_config(&AppConfig::load(conn))
}

/// Persist the **cloud** Settings submission — agent models + the two API
/// tokens. Two gates run before any write, so a rejected save persists nothing:
/// (1) every non-empty model slug is validated; (2) both API tokens must be
/// present after the update — saving is disabled while either is missing. The
/// gate is scoped to this submission alone (`docs/configuration.md §API
/// Tokens`); the provider credentials and local-suite config save ungated
/// through their own functions below. Models are then written in full (an empty
/// slug clears that agent's selection); each token is written only when the
/// update carries a non-empty value, leaving an untouched field's stored secret
/// in place.
pub fn save(conn: &Connection, models: &AgentModels, credentials: &CredentialUpdate) -> Result<()> {
    for (name, slug) in [
        ("Main Agent", &models.main),
        ("Bull Analyst", &models.bull),
        ("Bear Analyst", &models.bear),
        ("Balanced Analyst", &models.balanced),
    ] {
        let s = slug.trim();
        if !s.is_empty() && AgentModel::from_config_label(s).is_err() {
            bail!("{name}: unknown model {s:?}");
        }
    }

    // Token save-gate: saving is disabled unless BOTH cloud tokens are present
    // after this update — a non-blank entered value counts, otherwise the
    // already-stored value must be non-blank. Checked before any write so a gated
    // save leaves the store untouched. Scoped to this cloud submission only
    // (docs/configuration.md §API Tokens): the provider credentials save through
    // `save_provider_credentials`, outside this gate.
    let token_present = |key: &str, update: &Option<String>| -> Result<bool> {
        if update
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        {
            return Ok(true);
        }
        let stored = storage::get_setting(conn, key)?;
        Ok(stored
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty()))
    };
    if !token_present(KEY_OPENAI_API_KEY, &credentials.openai)? {
        bail!("OpenAI API token is required to save the configuration");
    }
    if !token_present(KEY_ANTHROPIC_API_KEY, &credentials.anthropic)? {
        bail!("Anthropic API token is required to save the configuration");
    }

    storage::set_setting(conn, KEY_MAIN_AGENT_MODEL, models.main.trim())?;
    storage::set_setting(conn, KEY_BULL_AGENT_MODEL, models.bull.trim())?;
    storage::set_setting(conn, KEY_BEAR_AGENT_MODEL, models.bear.trim())?;
    storage::set_setting(conn, KEY_BALANCED_AGENT_MODEL, models.balanced.trim())?;

    // Tokens: write only what the user actually entered; blank or absent leaves
    // the stored value untouched.
    put_secret(conn, KEY_OPENAI_API_KEY, &credentials.openai)?;
    put_secret(conn, KEY_ANTHROPIC_API_KEY, &credentials.anthropic)?;
    Ok(())
}

/// Write one secret only when the update carries a non-empty value; blank or
/// absent leaves the stored value untouched (a secret never re-displays, so a
/// re-save must not wipe it). Shared by the cloud and provider-credential saves.
fn put_secret(conn: &Connection, key: &str, val: &Option<String>) -> Result<()> {
    if let Some(v) = val {
        let v = v.trim();
        if !v.is_empty() {
            storage::set_setting(conn, key, v)?;
        }
    }
    Ok(())
}

/// Persist the external data-provider credentials (FMP / FRED / Tavily) —
/// deliberately **ungated**: these save with no cloud token present, the
/// local-suite setup path (`docs/configuration.md §External Data Provider
/// Credentials`). Same per-field semantics as the token save: only a non-empty
/// entered value writes.
pub fn save_provider_credentials(
    conn: &Connection,
    credentials: &ProviderCredentialUpdate,
) -> Result<()> {
    put_secret(conn, KEY_FMP_API_KEY, &credentials.fmp)?;
    put_secret(conn, KEY_FRED_API_KEY, &credentials.fred)?;
    put_secret(conn, KEY_TAVILY_API_KEY, &credentials.tavily)?;
    Ok(())
}

/// Persist the local-analysis-models config (`docs/configuration.md §Local
/// Models`) — **ungated**, like the provider credentials: presence of these
/// fields is what clears the proactive *local models not configured* warning,
/// so the save must never depend on a cloud token or on daemon connectivity.
/// All four values are written verbatim (trimmed; "" persists as a cleared
/// field, exactly like a cleared agent-model slug — these are not secrets, so
/// there is no leave-unchanged blank semantics).
///
/// One side effect guards the vector store: when the **embedder identity**
/// changes away from a previously configured value, the two local vector-memory
/// namespaces are cleared (`vector_memory::clear_local_namespaces`) — identity,
/// never dimension, is the compatibility key (`docs/storage.md §Local Vector
/// Memory`), so rows embedded under the old identity must never be searched
/// under the new one. Re-embedding from retained content is deferred (it needs
/// a reachable daemon, which this presence-only path must not require).
pub fn save_local_models(conn: &Connection, values: &LocalModelSettings) -> Result<()> {
    // The previously *effective* embedder identity (saved value, env fallback) —
    // compared against the new value before any write.
    let prior = AppConfig::load(conn);
    let prior_embedder = prior
        .local_embedder_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let new_embedder = values.embedder_model.trim();
    let identity_changed = prior_embedder.is_some_and(|old| old != new_embedder);

    // One transaction: the stale-cohort clear and the identity write commit (or
    // roll back) together. Committing the new id before the clear would let a
    // failed clear strand incompatible vectors permanently — a retry would read
    // the new id as unchanged and skip the cleanup forever.
    let tx = conn.unchecked_transaction()?;
    let cleared = if identity_changed {
        vector_memory::clear_local_namespaces(&tx)?
    } else {
        0
    };
    storage::set_setting(&tx, KEY_LOCAL_DAEMON_ENDPOINT, values.daemon_endpoint.trim())?;
    storage::set_setting(&tx, KEY_LOCAL_REASONER_MODEL, values.reasoner_model.trim())?;
    storage::set_setting(&tx, KEY_LOCAL_FAST_MODEL, values.fast_model.trim())?;
    storage::set_setting(&tx, KEY_LOCAL_EMBEDDER_MODEL, values.embedder_model.trim())?;
    tx.commit()?;

    if cleared > 0 {
        eprintln!(
            "local embedder changed ({} -> {new_embedder:?}): cleared {cleared} stale local vector-memory rows",
            prior_embedder.unwrap_or_default()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        conn
    }

    fn models(main: &str) -> AgentModels {
        AgentModels {
            main: main.into(),
            bull: "gpt-5".into(),
            bear: "gpt-5-mini".into(),
            balanced: "claude-sonnet".into(),
        }
    }

    #[test]
    fn available_models_lists_all_five_with_provider_grouping() {
        let opts = available_models();
        assert_eq!(opts.len(), 5);
        let gpt5 = opts
            .iter()
            .find(|o| o.slug == "gpt-5")
            .expect("gpt-5 option");
        assert_eq!(gpt5.label, "GPT-5");
        assert_eq!(gpt5.provider, "OpenAI");
        let opus = opts
            .iter()
            .find(|o| o.slug == "claude-opus")
            .expect("claude-opus option");
        assert_eq!(opus.provider, "Anthropic");
    }

    #[test]
    fn save_persists_models_and_secrets_and_load_view_reflects_them() {
        let conn = mem();
        save(
            &conn,
            &models("claude-opus"),
            &CredentialUpdate {
                openai: Some("sk-o".into()),
                anthropic: Some("sk-a".into()),
            },
        )
        .unwrap();
        save_provider_credentials(
            &conn,
            &ProviderCredentialUpdate {
                fmp: Some("fmp".into()),
                fred: Some("fred".into()),
                tavily: Some("tav".into()),
            },
        )
        .unwrap();
        // Raw storage holds the slug + the secret.
        assert_eq!(
            storage::get_setting(&conn, KEY_MAIN_AGENT_MODEL)
                .unwrap()
                .as_deref(),
            Some("claude-opus")
        );
        assert_eq!(
            storage::get_setting(&conn, KEY_OPENAI_API_KEY)
                .unwrap()
                .as_deref(),
            Some("sk-o")
        );
        // The view reflects the model selection and every configured flag (these
        // are env-independent: a saved value always wins in `AppConfig::load`).
        let view = load_view(&conn);
        assert_eq!(view.models.main, "claude-opus");
        let c = &view.credentials;
        assert!(c.openai && c.anthropic && c.fmp && c.fred && c.tavily);
    }

    #[test]
    fn view_never_serializes_a_raw_secret() {
        let cfg = AppConfig {
            openai_api_key: Some("sk-super-secret".into()),
            ..AppConfig::default()
        };
        let view = view_from_config(&cfg);
        assert!(view.credentials.openai);
        let json = serde_json::to_string(&view).unwrap();
        assert!(
            !json.contains("sk-super-secret"),
            "serialized view leaked the secret: {json}"
        );
    }

    #[test]
    fn save_leaves_a_stored_credential_unchanged_when_the_update_is_empty() {
        let conn = mem();
        // Both tokens stored so a token-less re-save passes the gate.
        storage::set_setting(&conn, KEY_OPENAI_API_KEY, "sk-existing").unwrap();
        storage::set_setting(&conn, KEY_ANTHROPIC_API_KEY, "sk-anthropic").unwrap();
        // No credential values supplied → the stored keys must survive.
        save(&conn, &models("gpt-5"), &CredentialUpdate::default()).unwrap();
        assert_eq!(
            storage::get_setting(&conn, KEY_OPENAI_API_KEY)
                .unwrap()
                .as_deref(),
            Some("sk-existing")
        );
        // A blank (whitespace) update is also a no-op.
        save(
            &conn,
            &models("gpt-5"),
            &CredentialUpdate {
                openai: Some("   ".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            storage::get_setting(&conn, KEY_OPENAI_API_KEY)
                .unwrap()
                .as_deref(),
            Some("sk-existing")
        );
    }

    #[test]
    fn save_is_rejected_when_a_required_api_token_is_missing() {
        let conn = mem();
        // Valid models, but no token stored or supplied → saving is disabled
        // (docs/configuration.md §API Tokens), and nothing is persisted.
        let err = save(&conn, &models("gpt-5"), &CredentialUpdate::default()).unwrap_err();
        assert!(err.to_string().contains("API token"), "{err}");
        assert_eq!(
            storage::get_setting(&conn, KEY_MAIN_AGENT_MODEL).unwrap(),
            None
        );
    }

    #[test]
    fn save_accepts_blank_token_fields_once_both_tokens_are_stored() {
        let conn = mem();
        storage::set_setting(&conn, KEY_OPENAI_API_KEY, "sk-o").unwrap();
        storage::set_setting(&conn, KEY_ANTHROPIC_API_KEY, "sk-a").unwrap();
        // Changing only models (token fields blank) is allowed because both
        // tokens are already configured.
        save(&conn, &models("claude-opus"), &CredentialUpdate::default()).unwrap();
        assert_eq!(
            storage::get_setting(&conn, KEY_MAIN_AGENT_MODEL)
                .unwrap()
                .as_deref(),
            Some("claude-opus")
        );
    }

    #[test]
    fn save_rejects_an_unknown_model_slug_without_writing_anything() {
        let conn = mem();
        let err = save(&conn, &models("not-a-model"), &CredentialUpdate::default()).unwrap_err();
        assert!(err.to_string().contains("not-a-model"), "{err}");
        // The bad slug aborted before any set_setting ran.
        assert_eq!(
            storage::get_setting(&conn, KEY_MAIN_AGENT_MODEL).unwrap(),
            None
        );
    }

    #[test]
    fn provider_credentials_save_with_no_cloud_token_present() {
        let conn = mem();
        // The split's headline behavior (docs/configuration.md §API Tokens): a
        // cloud-keyless machine persists FMP / FRED / Tavily — no token gate.
        save_provider_credentials(
            &conn,
            &ProviderCredentialUpdate {
                fmp: Some("fmp-key".into()),
                fred: Some("fred-key".into()),
                tavily: Some("tav-key".into()),
            },
        )
        .unwrap();
        assert_eq!(
            storage::get_setting(&conn, KEY_FMP_API_KEY)
                .unwrap()
                .as_deref(),
            Some("fmp-key")
        );
        // Blank / absent fields leave stored secrets untouched, like the tokens.
        save_provider_credentials(
            &conn,
            &ProviderCredentialUpdate {
                fmp: Some("  ".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            storage::get_setting(&conn, KEY_FMP_API_KEY)
                .unwrap()
                .as_deref(),
            Some("fmp-key")
        );
        // The cloud submission stays token-gated after the split.
        let err = save(&conn, &models("gpt-5"), &CredentialUpdate::default()).unwrap_err();
        assert!(err.to_string().contains("API token"), "{err}");
    }

    #[test]
    fn local_models_save_ungated_and_round_trip_in_the_view() {
        let conn = mem();
        // No cloud token anywhere — the local-suite setup path must still save.
        save_local_models(
            &conn,
            &LocalModelSettings {
                daemon_endpoint: " http://127.0.0.1:11434 ".into(),
                reasoner_model: "qwen3.5:122b".into(),
                fast_model: String::new(),
                embedder_model: "qwen3-embedding:4b".into(),
            },
        )
        .unwrap();
        // Values are trimmed on write and round-trip in the view (not secrets).
        let view = load_view(&conn);
        assert_eq!(view.local_models.daemon_endpoint, "http://127.0.0.1:11434");
        assert_eq!(view.local_models.reasoner_model, "qwen3.5:122b");
        assert_eq!(view.local_models.fast_model, "");
        assert_eq!(view.local_models.embedder_model, "qwen3-embedding:4b");
        // The presence gate clears on these fields alone (fast tier optional) —
        // provider credentials still warn separately until FMP/FRED are saved.
        let report = crate::local_model::local_presence_gate(&AppConfig::load(&conn));
        assert!(report
            .categories
            .iter()
            .all(|c| c.kind != crate::config::WarningKind::LocalModels));
    }

    #[test]
    fn changing_the_embedder_identity_clears_only_the_local_namespaces() {
        use crate::vector_memory::{insert_memory, MemoryKind, MemoryNamespace};
        let conn = mem();
        let values = |embedder: &str| LocalModelSettings {
            daemon_endpoint: "http://127.0.0.1:11434".into(),
            reasoner_model: "r".into(),
            fast_model: String::new(),
            embedder_model: embedder.into(),
        };
        save_local_models(&conn, &values("embed-a")).unwrap();
        for ns in [MemoryNamespace::Portfolio, MemoryNamespace::Opportunities] {
            insert_memory(&conn, MemoryKind::Learning, ns, None, "local", &[1.0], "2026-01-01")
                .unwrap();
        }
        insert_memory(
            &conn,
            MemoryKind::Learning,
            MemoryNamespace::Report,
            None,
            "report",
            &[1.0],
            "2026-01-01",
        )
        .unwrap();

        // Re-saving the same identity is not a change — nothing is cleared.
        save_local_models(&conn, &values("embed-a")).unwrap();
        let count = |ns| crate::vector_memory::count_memory(&conn, ns).unwrap();
        assert_eq!(count(MemoryNamespace::Portfolio), 1);

        // A changed identity clears both local namespaces, never the report's.
        save_local_models(&conn, &values("embed-b")).unwrap();
        assert_eq!(count(MemoryNamespace::Portfolio), 0);
        assert_eq!(count(MemoryNamespace::Opportunities), 0);
        assert_eq!(count(MemoryNamespace::Report), 1);
    }

    #[test]
    fn a_failed_stale_cohort_clear_rolls_back_the_identity_write() {
        use crate::vector_memory::{count_memory, insert_memory, MemoryKind, MemoryNamespace};
        let conn = mem();
        let values = |embedder: &str| LocalModelSettings {
            daemon_endpoint: "http://127.0.0.1:11434".into(),
            reasoner_model: "r".into(),
            fast_model: String::new(),
            embedder_model: embedder.into(),
        };
        save_local_models(&conn, &values("embed-a")).unwrap();
        insert_memory(
            &conn,
            MemoryKind::Learning,
            MemoryNamespace::Portfolio,
            None,
            "local",
            &[1.0],
            "2026-01-01",
        )
        .unwrap();

        // Force the clear to fail mid-transaction.
        conn.execute_batch(
            "CREATE TRIGGER vm_no_delete BEFORE DELETE ON vector_memory
             BEGIN SELECT RAISE(ABORT, 'delete blocked'); END",
        )
        .unwrap();
        let err = save_local_models(&conn, &values("embed-b")).unwrap_err();
        assert!(err.to_string().contains("delete blocked"), "{err}");
        // The rollback held: the OLD identity is still stored — a later retry
        // still reads the change and re-attempts the clear (committing the new
        // id here would make the retry skip the cleanup forever) — and the
        // stale rows are still present, never half-cleared.
        assert_eq!(
            storage::get_setting(&conn, KEY_LOCAL_EMBEDDER_MODEL)
                .unwrap()
                .as_deref(),
            Some("embed-a")
        );
        assert_eq!(count_memory(&conn, MemoryNamespace::Portfolio).unwrap(), 1);

        // Once the fault clears, the retry lands the clear and the write together.
        conn.execute("DROP TRIGGER vm_no_delete", []).unwrap();
        save_local_models(&conn, &values("embed-b")).unwrap();
        assert_eq!(
            storage::get_setting(&conn, KEY_LOCAL_EMBEDDER_MODEL)
                .unwrap()
                .as_deref(),
            Some("embed-b")
        );
        assert_eq!(count_memory(&conn, MemoryNamespace::Portfolio).unwrap(), 0);
    }

    #[test]
    fn save_stores_an_empty_slug_to_clear_a_selection() {
        let conn = mem();
        // Tokens supplied so the save passes the token gate; the point under test
        // is that an empty model slug persists as "" (a cleared selection).
        save(
            &conn,
            &AgentModels::default(), // all four empty
            &CredentialUpdate {
                openai: Some("sk-o".into()),
                anthropic: Some("sk-a".into()),
            },
        )
        .unwrap();
        assert_eq!(
            storage::get_setting(&conn, KEY_MAIN_AGENT_MODEL)
                .unwrap()
                .as_deref(),
            Some("")
        );
        // A fully-empty agent configuration is still blocked by the gate
        // (env-independent: every agent reads as "no model selected").
        assert!(crate::config::validate(&AppConfig::load(&conn)).is_blocked);
    }
}
