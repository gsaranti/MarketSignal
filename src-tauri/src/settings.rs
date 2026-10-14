//! The Settings store: persist user configuration into `app_settings` and project it
//! back for the Settings view (`docs/configuration.md`, `docs/interface.md §Settings`);
//! the `#[tauri::command]` wrappers live in `lib.rs`. A secret never round-trips to
//! the webview: `view_from_config` collapses each credential to a `configured`
//! boolean. The cloud submission (`save`) is token-gated; the provider-credential,
//! local-model and web-research saves are ungated so a cloud-keyless machine can
//! complete local-suite setup (`docs/configuration.md §API Tokens`).

use anyhow::{bail, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::config::{
    AppConfig, KEY_ANTHROPIC_API_KEY, KEY_BALANCED_AGENT_MODEL, KEY_BEAR_AGENT_MODEL,
    KEY_BULL_AGENT_MODEL, KEY_FMP_API_KEY, KEY_FRED_API_KEY, KEY_LOCAL_DAEMON_ENDPOINT,
    KEY_LOCAL_FAST_MODEL, KEY_LOCAL_REASONER_MODEL,
    KEY_MAIN_AGENT_MODEL, KEY_OPENAI_API_KEY, KEY_SEARXNG_ENDPOINT, KEY_TAVILY_API_KEY,
};
use crate::model_agent::AgentModel;
use crate::storage;

/// One Settings dropdown option; `provider` is the group it lists under.
#[derive(Debug, Clone, Serialize)]
pub struct ModelOption {
    pub slug: String,
    pub label: String,
    pub provider: String,
}

/// Each the persisted slug, or "" when unset; the form submits them back verbatim.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentModels {
    pub main: String,
    pub bull: String,
    pub bear: String,
    pub balanced: String,
}

/// Whether each credential is configured, never the value itself, so no secret
/// leaves the backend.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CredentialStatus {
    pub openai: bool,
    pub anthropic: bool,
    pub fmp: bool,
    pub fred: bool,
    pub tavily: bool,
}

/// The local-analysis-models values (`docs/configuration.md §Local Models`). Not
/// secrets, so unlike credentials they round-trip in full ("" when unset).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalModelSettings {
    pub daemon_endpoint: String,
    pub reasoner_model: String,
    pub fast_model: String,
}

/// The web-research settings (`docs/configuration.md §Web Research`). Not a secret,
/// so it round-trips in full.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebResearchSettings {
    pub searxng_endpoint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub models: AgentModels,
    pub credentials: CredentialStatus,
    pub local_models: LocalModelSettings,
    pub web_research: WebResearchSettings,
    pub available_models: Vec<ModelOption>,
}

/// The API-token half of the cloud save request. `Some` only when the user entered
/// a new value; `None` or blank leaves the stored value unchanged, so a re-save never
/// wipes a key the form never re-displays. The provider credentials save through
/// [`ProviderCredentialUpdate`], outside the token gate
/// (`docs/configuration.md §API Tokens`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CredentialUpdate {
    pub openai: Option<String>,
    pub anthropic: Option<String>,
}

/// Independent of the token-gated cloud submission, so a cloud-keyless machine can
/// persist these for the local suite (`docs/configuration.md §External Data Provider
/// Credentials`). Same semantics as [`CredentialUpdate`]: `None` or blank leaves the
/// stored secret unchanged.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProviderCredentialUpdate {
    pub fmp: Option<String>,
    pub fred: Option<String>,
    pub tavily: Option<String>,
}

/// Sourced from `AgentModel` so slugs and display names have a single backend home.
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
        },
        web_research: WebResearchSettings {
            searxng_endpoint: cfg.searxng_endpoint.clone().unwrap_or_default(),
        },
        available_models: available_models(),
    }
}

pub fn load_view(conn: &Connection) -> SettingsView {
    view_from_config(&AppConfig::load(conn))
}

/// The cloud submission: agent models plus the two API tokens. Two gates run before
/// any write, so a rejected save persists nothing: every non-empty model slug must
/// parse, and both API tokens must be present after the update
/// (`docs/configuration.md §API Tokens`). An empty slug clears that agent's
/// selection; a token is written only when the update carries a non-empty value.
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

    // A non-blank entered value counts, otherwise the stored value must be
    // non-blank. Checked before any write so a gated save leaves the store
    // untouched.
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

    put_secret(conn, KEY_OPENAI_API_KEY, &credentials.openai)?;
    put_secret(conn, KEY_ANTHROPIC_API_KEY, &credentials.anthropic)?;
    Ok(())
}

/// Writes only a non-empty value; blank or absent leaves the stored value untouched,
/// since a secret never re-displays and a re-save must not wipe it.
fn put_secret(conn: &Connection, key: &str, val: &Option<String>) -> Result<()> {
    if let Some(v) = val {
        let v = v.trim();
        if !v.is_empty() {
            storage::set_setting(conn, key, v)?;
        }
    }
    Ok(())
}

/// Deliberately ungated: these save with no cloud token present, the local-suite
/// setup path (`docs/configuration.md §External Data Provider Credentials`).
pub fn save_provider_credentials(
    conn: &Connection,
    credentials: &ProviderCredentialUpdate,
) -> Result<()> {
    put_secret(conn, KEY_FMP_API_KEY, &credentials.fmp)?;
    put_secret(conn, KEY_FRED_API_KEY, &credentials.fred)?;
    put_secret(conn, KEY_TAVILY_API_KEY, &credentials.tavily)?;
    Ok(())
}

/// Ungated: presence of these fields is what clears the local-models warning, so the
/// save must never depend on a cloud token or on daemon connectivity
/// (`docs/configuration.md §Local Models`). Written verbatim; "" persists as a
/// cleared field, since these are not secrets.
pub fn save_local_models(conn: &Connection, values: &LocalModelSettings) -> Result<()> {
    storage::set_setting(conn, KEY_LOCAL_DAEMON_ENDPOINT, values.daemon_endpoint.trim())?;
    storage::set_setting(conn, KEY_LOCAL_REASONER_MODEL, values.reasoner_model.trim())?;
    storage::set_setting(conn, KEY_LOCAL_FAST_MODEL, values.fast_model.trim())?;
    Ok(())
}

/// Ungated: SearXNG is off every gate, so saving its endpoint must never depend on a
/// token or on reachability (`docs/configuration.md §Web Research`). Written
/// verbatim; "" persists as cleared.
pub fn save_web_research(conn: &Connection, values: &WebResearchSettings) -> Result<()> {
    storage::set_setting(conn, KEY_SEARXNG_ENDPOINT, values.searxng_endpoint.trim())?;
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
        save(&conn, &models("gpt-5"), &CredentialUpdate::default()).unwrap();
        assert_eq!(
            storage::get_setting(&conn, KEY_OPENAI_API_KEY)
                .unwrap()
                .as_deref(),
            Some("sk-existing")
        );
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
        assert_eq!(
            storage::get_setting(&conn, KEY_MAIN_AGENT_MODEL).unwrap(),
            None
        );
    }

    #[test]
    fn provider_credentials_save_with_no_cloud_token_present() {
        let conn = mem();
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
        let err = save(&conn, &models("gpt-5"), &CredentialUpdate::default()).unwrap_err();
        assert!(err.to_string().contains("API token"), "{err}");
    }

    #[test]
    fn local_models_save_ungated_and_round_trip_in_the_view() {
        let conn = mem();
        save_local_models(
            &conn,
            &LocalModelSettings {
                daemon_endpoint: " http://127.0.0.1:11434 ".into(),
                reasoner_model: "qwen3.5:122b".into(),
                fast_model: String::new(),
            },
        )
        .unwrap();
        let view = load_view(&conn);
        assert_eq!(view.local_models.daemon_endpoint, "http://127.0.0.1:11434");
        assert_eq!(view.local_models.reasoner_model, "qwen3.5:122b");
        assert_eq!(view.local_models.fast_model, "");
        // Only the LocalModels category is checked: the provider credentials still
        // warn separately until they are saved.
        let report = crate::local_model::local_presence_gate(&AppConfig::load(&conn));
        assert!(report
            .categories
            .iter()
            .all(|c| c.kind != crate::config::WarningKind::LocalModels));
    }

    #[test]
    fn web_research_saves_ungated_and_round_trips_in_the_view() {
        let conn = mem();
        save_web_research(
            &conn,
            &WebResearchSettings {
                searxng_endpoint: " http://127.0.0.1:8888 ".into(),
            },
        )
        .unwrap();
        let view = load_view(&conn);
        assert_eq!(view.web_research.searxng_endpoint, "http://127.0.0.1:8888");
        save_web_research(
            &conn,
            &WebResearchSettings {
                searxng_endpoint: String::new(),
            },
        )
        .unwrap();
        assert_eq!(load_view(&conn).web_research.searxng_endpoint, "");
    }

    #[test]
    fn save_stores_an_empty_slug_to_clear_a_selection() {
        let conn = mem();
        // Tokens supplied so the save passes the token gate.
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
        assert!(crate::config::validate(&AppConfig::load(&conn)).is_blocked);
    }
}
