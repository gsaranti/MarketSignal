//! The Settings store: persist user configuration — agent models, API tokens,
//! and provider credentials — into `app_settings`, and project it back for the
//! Settings view (`docs/configuration.md`, `docs/interface.md §Settings`).
//!
//! Two halves, mirroring `research`: the pure logic lives here; the thin
//! `#[tauri::command]` wrappers live in `lib.rs`. One load-bearing rule: a secret
//! never round-trips to the webview. `view_from_config` collapses each credential
//! to a `configured` boolean and never carries the stored key. Model slugs are
//! not secret, so they are returned for the dropdowns to pre-select.

use anyhow::{bail, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::config::{
    AppConfig, KEY_ANTHROPIC_API_KEY, KEY_BALANCED_AGENT_MODEL, KEY_BEAR_AGENT_MODEL,
    KEY_BULL_AGENT_MODEL, KEY_FMP_API_KEY, KEY_FRED_API_KEY, KEY_MAIN_AGENT_MODEL,
    KEY_OPENAI_API_KEY, KEY_TAVILY_API_KEY,
};
use crate::model_agent::AgentModel;
use crate::storage;

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

/// What the Settings view renders: the current model selections, the
/// per-credential configured flags, and the model dropdown's options.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub models: AgentModels,
    pub credentials: CredentialStatus,
    pub available_models: Vec<ModelOption>,
}

/// The credential half of a save request. Each field is `Some` only when the user
/// entered a new value; `None` (or a blank string) means "leave the stored value
/// unchanged" — so a re-save never wipes a key the form never re-displays.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CredentialUpdate {
    pub openai: Option<String>,
    pub anthropic: Option<String>,
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
        available_models: available_models(),
    }
}

/// Load the Settings view from the persisted store (env fallback per field).
pub fn load_view(conn: &Connection) -> SettingsView {
    view_from_config(&AppConfig::load(conn))
}

/// Persist a Settings submission. Two gates run before any write, so a rejected
/// save persists nothing: (1) every non-empty model slug is validated; (2) both
/// API tokens must be present after the update — saving is disabled while either
/// is missing (`docs/configuration.md §API Tokens`). Models are then written in
/// full (an empty slug clears that agent's selection); each credential is written
/// only when the update carries a non-empty value, leaving an untouched field's
/// stored secret in place.
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

    // Token save-gate: saving is disabled unless BOTH provider tokens are present
    // after this update — a non-blank entered value counts, otherwise the
    // already-stored value must be non-blank. Checked before any write so a gated
    // save leaves the store untouched.
    let token_present = |key: &str, update: &Option<String>| -> Result<bool> {
        if update.as_deref().map(str::trim).is_some_and(|s| !s.is_empty()) {
            return Ok(true);
        }
        let stored = storage::get_setting(conn, key)?;
        Ok(stored.as_deref().map(str::trim).is_some_and(|s| !s.is_empty()))
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

    // Credentials: write only what the user actually entered; blank or absent
    // leaves the stored value untouched.
    let put = |key: &str, val: &Option<String>| -> Result<()> {
        if let Some(v) = val {
            let v = v.trim();
            if !v.is_empty() {
                storage::set_setting(conn, key, v)?;
            }
        }
        Ok(())
    };
    put(KEY_OPENAI_API_KEY, &credentials.openai)?;
    put(KEY_ANTHROPIC_API_KEY, &credentials.anthropic)?;
    put(KEY_FMP_API_KEY, &credentials.fmp)?;
    put(KEY_FRED_API_KEY, &credentials.fred)?;
    put(KEY_TAVILY_API_KEY, &credentials.tavily)?;
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
        let gpt5 = opts.iter().find(|o| o.slug == "gpt-5").expect("gpt-5 option");
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
                ..Default::default()
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
