//! The pre-run execution gate: structured configuration validation.
//!
//! `docs/report-workflow.md §Step 1` and `docs/configuration.md` require
//! that no report run starts until the configuration is complete — all four
//! agent models selected, both provider API tokens present, and both external
//! data-provider credentials present. This module turns that requirement into a
//! pure function: `validate(&AppConfig) -> ValidationReport`. The report drives
//! both the backend block (the Tauri command refuses to run when blocked) and
//! the frontend Persistent Warning Area (`docs/interface.md`).
//!
//! `AppConfig` is the config substrate. `load` reads the persisted Settings
//! store (`app_settings`, written by `settings::save`) with the environment
//! variables as a per-field fallback, so the env-based live smoke keeps working
//! until a value is saved; `from_env` is the env-only path the smoke and the
//! gate-bypassing adapter use. The store read is confined to these two
//! constructors; `validate` is pure over the struct and never touches the
//! environment or the database, so the pass/block matrix is unit-testable
//! without env mutation.
//!
//! Scope note: the non-blocking failed-job warning category is modeled in
//! `WarningKind` but produced from job history by `jobs::failure_warning`, not
//! here. Network reachability is not a gate check — an unreachable provider
//! surfaces as a job *failure* (`docs/scheduling.md §Offline Behavior`), not a
//! pre-run block.

use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::agent::Posture;
use crate::model_agent::{AgentModel, MainAgentConfig, Provider};
use crate::storage;

/// `app_settings` keys backing the persisted configuration. `settings::save`
/// writes them; `AppConfig::load` reads them with the env vars as a per-field
/// fallback. The slugs deliberately match the `AppConfig` field names.
pub const KEY_MAIN_AGENT_MODEL: &str = "main_agent_model";
pub const KEY_BULL_AGENT_MODEL: &str = "bull_agent_model";
pub const KEY_BEAR_AGENT_MODEL: &str = "bear_agent_model";
pub const KEY_BALANCED_AGENT_MODEL: &str = "balanced_agent_model";
pub const KEY_OPENAI_API_KEY: &str = "openai_api_key";
pub const KEY_ANTHROPIC_API_KEY: &str = "anthropic_api_key";
pub const KEY_FMP_API_KEY: &str = "fmp_api_key";
pub const KEY_FRED_API_KEY: &str = "fred_api_key";
pub const KEY_TAVILY_API_KEY: &str = "tavily_api_key";

/// Local analysis suite settings (`docs/configuration.md §Local Analysis Suite
/// Configuration`, `docs/local-models.md`). These gate the **local jobs only** and
/// are deliberately absent from `validate` — the cloud-report gate is independent, so
/// a machine set up for one need not be set up for the other.
pub const KEY_LOCAL_DAEMON_ENDPOINT: &str = "local_daemon_endpoint";
pub const KEY_LOCAL_REASONER_MODEL: &str = "local_reasoner_model";
pub const KEY_LOCAL_FAST_MODEL: &str = "local_fast_model";
pub const KEY_LOCAL_EMBEDDER_MODEL: &str = "local_embedder_model";

/// The Charles Schwab developer app's client id (`docs/schwab-integration.md`). An
/// identifier, not a bearer secret, so it rides the SQLite `app_settings` store like
/// the other non-sensitive config — the *client secret* and OAuth tokens live on the
/// Keychain rail (`crate::schwab_secrets`) instead. Local-suite config: absent from the
/// cloud `validate` gate.
pub const KEY_SCHWAB_CLIENT_ID: &str = "schwab_client_id";

/// The four de-duplicating Persistent Warning Area categories (walk Q4,
/// `docs/interface.md §Persistent Warning Area`). The three configuration
/// categories are produced by `validate`; the one job category (`FailedJob`) is
/// produced from job history and is modeled here so the warning structure is whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningKind {
    AgentConfiguration,
    ApiTokens,
    ProviderCredentials,
    FailedJob,
    /// The local analysis suite's own gate gap (daemon unreachable, roster
    /// incomplete, or local config not yet set). Produced by `local_model::local_gate`,
    /// never by `validate`; modeled here so the warning structure stays whole and the
    /// frontend can render it identically (`docs/local-models.md §Failure posture`).
    LocalModels,
    /// The local analysis suite's Schwab-connection gap: no connected account, or the
    /// 7-day refresh token has lapsed (`docs/schwab-integration.md §A connected Schwab
    /// account is required`). Like `LocalModels`, it gates the **local jobs only** and is
    /// produced where the Keychain token rail is reachable, never by the cloud-report
    /// `validate` (which is pure over `AppConfig` and cannot see the tokens); modeled here
    /// so the warning structure stays whole.
    Schwab,
}

impl WarningKind {
    /// Whether a warning of this kind blocks a run. The three configuration
    /// gaps block (a run cannot proceed without them); the job categories are
    /// informational history, not gate failures.
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            WarningKind::AgentConfiguration
                | WarningKind::ApiTokens
                | WarningKind::ProviderCredentials
                | WarningKind::LocalModels
                | WarningKind::Schwab
        )
    }
}

/// One warning category for display: its kind, a human-readable title used as
/// the Persistent Warning Area row label, and the concrete missing items.
///
/// `dismiss_id` is the *identity* of the specific warning being shown, for the one
/// dismissible (non-blocking) category — the failed run's id. The frontend echoes it
/// back to `dismiss_warning` so the dismissal targets the rendered warning, not
/// whatever the backend would re-derive as current at click time
/// (`docs/interface.md §Persistent Warning Area` — a later event must still surface
/// fresh). `None` for the blocking config categories, which are gate state and carry
/// no dismiss control.
#[derive(Debug, Clone, Serialize)]
pub struct WarningCategory {
    pub kind: WarningKind,
    pub title: String,
    pub items: Vec<String>,
    pub dismiss_id: Option<String>,
}

/// The result of validating the configuration: the active warning categories
/// (only non-empty ones are included) and whether any of them blocks a run.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub categories: Vec<WarningCategory>,
    pub is_blocked: bool,
}

/// The configuration substrate. Each field is `None` when neither the persisted
/// store nor its env var carries a value; blank values are treated as unset by
/// `present`. Built by `from_env` (env only) or `load` (store, env-fallback).
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    pub main_agent_model: Option<String>,
    pub bull_agent_model: Option<String>,
    pub bear_agent_model: Option<String>,
    pub balanced_agent_model: Option<String>,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub fmp_api_key: Option<String>,
    pub fred_api_key: Option<String>,
    pub tavily_api_key: Option<String>,
    /// Local analysis suite config (`docs/local-models.md`). Read by
    /// `local_model::{endpoint_from_config, roster_from_config, local_gate}`; not part
    /// of the cloud `validate` gate.
    pub local_daemon_endpoint: Option<String>,
    pub local_reasoner_model: Option<String>,
    pub local_fast_model: Option<String>,
    pub local_embedder_model: Option<String>,
    /// Schwab developer app client id (`docs/schwab-integration.md`). Read by the
    /// `schwab_connect` command; not part of the cloud `validate` gate.
    pub schwab_client_id: Option<String>,
}

/// A set-and-non-blank value, or `None`. An env var set to "" is effectively
/// unset for gate purposes, so it must not pass validation. `pub(crate)` so the
/// local-suite gate (`local_model`) reuses the same blank-is-unset rule.
pub(crate) fn present(opt: &Option<String>) -> Option<&str> {
    opt.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

impl AppConfig {
    /// Read the configuration from the environment. Variable names follow the
    /// existing `MARKET_SIGNAL_MAIN_AGENT_MODEL` / `OPENAI_API_KEY` convention.
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok();
        Self {
            main_agent_model: get("MARKET_SIGNAL_MAIN_AGENT_MODEL"),
            bull_agent_model: get("MARKET_SIGNAL_BULL_AGENT_MODEL"),
            bear_agent_model: get("MARKET_SIGNAL_BEAR_AGENT_MODEL"),
            balanced_agent_model: get("MARKET_SIGNAL_BALANCED_AGENT_MODEL"),
            openai_api_key: get("OPENAI_API_KEY"),
            anthropic_api_key: get("ANTHROPIC_API_KEY"),
            fmp_api_key: get("FMP_API_KEY"),
            fred_api_key: get("FRED_API_KEY"),
            tavily_api_key: get("TAVILY_API_KEY"),
            local_daemon_endpoint: get("MARKET_SIGNAL_LOCAL_DAEMON_ENDPOINT"),
            local_reasoner_model: get("MARKET_SIGNAL_LOCAL_REASONER_MODEL"),
            local_fast_model: get("MARKET_SIGNAL_LOCAL_FAST_MODEL"),
            local_embedder_model: get("MARKET_SIGNAL_LOCAL_EMBEDDER_MODEL"),
            schwab_client_id: get("MARKET_SIGNAL_SCHWAB_CLIENT_ID"),
        }
    }

    /// Read the configuration from the persisted Settings store, falling back to
    /// the environment per field: a saved `app_settings` value wins, an unset key
    /// (or a read error) defers to the env var. So a fresh install with no saved
    /// settings behaves exactly like `from_env`, and the env-based live smoke
    /// keeps working until the user saves something in Settings.
    pub fn load(conn: &Connection) -> Self {
        let env = Self::from_env();
        // Some(saved) wins; None (unset key or read error) falls back to env.
        let saved = |key: &str, fallback: Option<String>| -> Option<String> {
            storage::get_setting(conn, key).ok().flatten().or(fallback)
        };
        Self {
            main_agent_model: saved(KEY_MAIN_AGENT_MODEL, env.main_agent_model),
            bull_agent_model: saved(KEY_BULL_AGENT_MODEL, env.bull_agent_model),
            bear_agent_model: saved(KEY_BEAR_AGENT_MODEL, env.bear_agent_model),
            balanced_agent_model: saved(KEY_BALANCED_AGENT_MODEL, env.balanced_agent_model),
            openai_api_key: saved(KEY_OPENAI_API_KEY, env.openai_api_key),
            anthropic_api_key: saved(KEY_ANTHROPIC_API_KEY, env.anthropic_api_key),
            fmp_api_key: saved(KEY_FMP_API_KEY, env.fmp_api_key),
            fred_api_key: saved(KEY_FRED_API_KEY, env.fred_api_key),
            tavily_api_key: saved(KEY_TAVILY_API_KEY, env.tavily_api_key),
            local_daemon_endpoint: saved(KEY_LOCAL_DAEMON_ENDPOINT, env.local_daemon_endpoint),
            local_reasoner_model: saved(KEY_LOCAL_REASONER_MODEL, env.local_reasoner_model),
            local_fast_model: saved(KEY_LOCAL_FAST_MODEL, env.local_fast_model),
            local_embedder_model: saved(KEY_LOCAL_EMBEDDER_MODEL, env.local_embedder_model),
            schwab_client_id: saved(KEY_SCHWAB_CLIENT_ID, env.schwab_client_id),
        }
    }

    /// Resolve the Main Agent's adapter config from validated configuration:
    /// the selected model and the API key for that model's provider. Used by the
    /// command once the gate has passed, and by the live smoke via
    /// `ModelMainAgent::from_env`. Errors mirror the gate's wording so a caller
    /// that bypasses the gate still gets a legible message.
    pub fn main_agent_config(&self) -> Result<MainAgentConfig> {
        let label = present(&self.main_agent_model).ok_or_else(|| {
            anyhow!("MARKET_SIGNAL_MAIN_AGENT_MODEL is not set (no Main Agent model selected)")
        })?;
        let model = AgentModel::from_config_label(label)?;
        let (key_opt, var) = match model.provider() {
            Provider::OpenAi => (&self.openai_api_key, "OPENAI_API_KEY"),
            Provider::Anthropic => (&self.anthropic_api_key, "ANTHROPIC_API_KEY"),
        };
        let api_key = present(key_opt)
            .ok_or_else(|| {
                anyhow!("{var} is not set (required for the selected Main Agent model)")
            })?
            .to_string();
        Ok(MainAgentConfig { model, api_key })
    }

    /// Resolve one analyst adapter's config (`docs/configuration.md §Agent Model
    /// Configuration`): the user-selected model for the given [`Posture`] and the API
    /// key for that model's provider. Mirrors `main_agent_config` — same post-gate
    /// resolution and error wording — but keyed by posture, since each of the three
    /// analysts (Bull / Bear / Balanced) is independently configurable. Reuses
    /// `MainAgentConfig` ({model, api_key}); the posture is carried by the adapter, not
    /// the config.
    pub fn analyst_config(&self, posture: Posture) -> Result<MainAgentConfig> {
        let (label_opt, var) = match posture {
            Posture::Bull => (&self.bull_agent_model, "MARKET_SIGNAL_BULL_AGENT_MODEL"),
            Posture::Bear => (&self.bear_agent_model, "MARKET_SIGNAL_BEAR_AGENT_MODEL"),
            Posture::Balanced => (
                &self.balanced_agent_model,
                "MARKET_SIGNAL_BALANCED_AGENT_MODEL",
            ),
        };
        let name = posture.display_name();
        let label = present(label_opt)
            .ok_or_else(|| anyhow!("{var} is not set (no {name} model selected)"))?;
        let model = AgentModel::from_config_label(label)?;
        let (key_opt, key_var) = match model.provider() {
            Provider::OpenAi => (&self.openai_api_key, "OPENAI_API_KEY"),
            Provider::Anthropic => (&self.anthropic_api_key, "ANTHROPIC_API_KEY"),
        };
        let api_key = present(key_opt)
            .ok_or_else(|| {
                anyhow!("{key_var} is not set (required for the selected {name} model)")
            })?
            .to_string();
        Ok(MainAgentConfig { model, api_key })
    }

    /// The FMP API key for the baseline market-data scan
    /// (`docs/report-workflow.md §Step 3`), resolved from validated configuration. Mirrors
    /// `main_agent_config`'s post-gate resolution: after a passing `validate` the
    /// credential is present, so the error arm is defensive.
    pub fn fmp_key(&self) -> Result<String> {
        Ok(present(&self.fmp_api_key)
            .ok_or_else(|| {
                anyhow!("FMP_API_KEY is not set (required for the baseline market-data scan)")
            })?
            .to_string())
    }

    /// The FRED API key for the macro / commodity half of the baseline scan
    /// (`docs/report-workflow.md §Step 3`), resolved from validated
    /// configuration. Mirrors `fmp_key`: after a passing `validate` the credential
    /// is present, so the error arm is defensive.
    pub fn fred_key(&self) -> Result<String> {
        Ok(present(&self.fred_api_key)
            .ok_or_else(|| {
                anyhow!("FRED_API_KEY is not set (required for the baseline market-data scan)")
            })?
            .to_string())
    }

    /// The Tavily API key for Step-7 news ingestion (`docs/report-workflow
    /// .md §Step 7`), resolved from validated configuration. Mirrors `fmp_key` /
    /// `fred_key`: after a passing `validate` the credential is present (Tavily is
    /// a required provider credential), so the error arm is defensive.
    pub fn tavily_key(&self) -> Result<String> {
        Ok(present(&self.tavily_api_key)
            .ok_or_else(|| {
                anyhow!("TAVILY_API_KEY is not set (required for Step-7 news ingestion)")
            })?
            .to_string())
    }

    /// The OpenAI API key for the fixed internal headline-filter model (GPT-5
    /// mini, `docs/agents.md §Headline Filtering`), resolved from validated
    /// configuration. Distinct from `main_agent_config`, which resolves the
    /// user-selected agent model's provider key — this is for the non-configurable
    /// internal stages, which always use OpenAI. After a passing `validate` the
    /// OpenAI token is present (always required), so the error arm is defensive.
    pub fn openai_key(&self) -> Result<String> {
        Ok(present(&self.openai_api_key)
            .ok_or_else(|| {
                anyhow!("OPENAI_API_KEY is not set (required for the fixed internal headline-filter model)")
            })?
            .to_string())
    }

    /// The Anthropic API key for the fixed internal research-routing model (Claude
    /// Sonnet, `docs/agents.md §Research Routing`), resolved from validated
    /// configuration. Like `openai_key`, this is for a non-configurable internal
    /// stage, distinct from `main_agent_config`'s user-selected agent key. After a
    /// passing `validate` the Anthropic token is present (always required, since the
    /// fixed internal stages span both providers), so the error arm is defensive.
    pub fn anthropic_key(&self) -> Result<String> {
        Ok(present(&self.anthropic_api_key)
            .ok_or_else(|| {
                anyhow!("ANTHROPIC_API_KEY is not set (required for the fixed internal research-routing model)")
            })?
            .to_string())
    }
}

/// One configured agent slot, paired with its display name for warning copy.
struct AgentSlot<'a> {
    name: &'a str,
    value: &'a Option<String>,
}

/// Validate the configuration against the execution gate. Pure: no I/O, no env
/// access — every input is on `cfg`. Only non-empty categories are returned, so
/// a clean configuration yields an empty `categories` and `is_blocked == false`.
/// Join names into a readable Oxford-comma list: "A", "A and B", "A, B, and C".
/// Keeps each warning category to one scannable sentence instead of one row per
/// missing item. `pub(crate)` so the local-suite gate (`local_model`) renders its
/// items the same way.
pub(crate) fn join_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [a] => (*a).to_string(),
        [a, b] => format!("{a} and {b}"),
        _ => {
            let (last, rest) = items.split_last().expect("non-empty slice");
            format!("{}, and {}", rest.join(", "), last)
        }
    }
}

pub fn validate(cfg: &AppConfig) -> ValidationReport {
    let mut categories: Vec<WarningCategory> = Vec::new();

    // Agent configuration: all four agents need a present, parseable model.
    let slots = [
        AgentSlot {
            name: "Main Agent",
            value: &cfg.main_agent_model,
        },
        AgentSlot {
            name: "Bull Analyst",
            value: &cfg.bull_agent_model,
        },
        AgentSlot {
            name: "Bear Analyst",
            value: &cfg.bear_agent_model,
        },
        AgentSlot {
            name: "Balanced Analyst",
            value: &cfg.balanced_agent_model,
        },
    ];
    // One concise line per problem rather than one row per agent, so the warning
    // area reads as a brief status, not a wall of repeated predicates.
    let mut not_selected: Vec<&str> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for slot in slots {
        match present(slot.value) {
            None => not_selected.push(slot.name),
            Some(label) => {
                if AgentModel::from_config_label(label).is_err() {
                    unknown.push(format!("{} (\"{label}\")", slot.name));
                }
            }
        }
    }
    let mut agent_items = Vec::new();
    if !not_selected.is_empty() {
        agent_items.push(format!(
            "No model selected for {}.",
            join_list(&not_selected)
        ));
    }
    if !unknown.is_empty() {
        agent_items.push(format!("Unknown model for {}.", unknown.join(", ")));
    }
    if !agent_items.is_empty() {
        categories.push(WarningCategory {
            kind: WarningKind::AgentConfiguration,
            title: "Agent configuration".to_string(),
            items: agent_items,
            dismiss_id: None,
        });
    }

    // API tokens: both are always required (the fixed internal stages span both
    // providers — docs/configuration.md §API Tokens).
    let mut missing_tokens: Vec<&str> = Vec::new();
    if present(&cfg.openai_api_key).is_none() {
        missing_tokens.push("OpenAI");
    }
    if present(&cfg.anthropic_api_key).is_none() {
        missing_tokens.push("Anthropic");
    }
    if !missing_tokens.is_empty() {
        categories.push(WarningCategory {
            kind: WarningKind::ApiTokens,
            title: "API tokens".to_string(),
            items: vec![format!("Missing for {}.", join_list(&missing_tokens))],
            dismiss_id: None,
        });
    }

    // External data-provider credentials: FMP, FRED, and Tavily are all required
    // to run (docs/configuration.md §External Data Provider Credentials). FRED
    // joined the gate when its adapter landed — it now sources non-optional Step-3
    // baseline series (Treasury yields, the dollar index, commodities).
    let mut missing_creds: Vec<&str> = Vec::new();
    if present(&cfg.fmp_api_key).is_none() {
        missing_creds.push("Financial Modeling Prep");
    }
    if present(&cfg.fred_api_key).is_none() {
        missing_creds.push("FRED");
    }
    if present(&cfg.tavily_api_key).is_none() {
        missing_creds.push("Tavily");
    }
    if !missing_creds.is_empty() {
        categories.push(WarningCategory {
            kind: WarningKind::ProviderCredentials,
            title: "Provider credentials".to_string(),
            items: vec![format!("Missing for {}.", join_list(&missing_creds))],
            dismiss_id: None,
        });
    }

    let is_blocked = categories.iter().any(|c| c.kind.is_blocking());
    ValidationReport {
        categories,
        is_blocked,
    }
}

/// A concise one-line reason a run was blocked, for the command's error return.
/// The structured detail lives in the `ValidationReport` the frontend already
/// shows in the Persistent Warning Area; this is only the fallback summary.
pub fn blocked_summary(report: &ValidationReport) -> String {
    let titles: Vec<&str> = report
        .categories
        .iter()
        .filter(|c| c.kind.is_blocking())
        .map(|c| c.title.as_str())
        .collect();
    format!(
        "Cannot generate report — resolve the configuration warnings first: {}.",
        titles.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fully-configured AppConfig — the all-green baseline each test perturbs.
    fn complete() -> AppConfig {
        AppConfig {
            main_agent_model: Some("claude-opus".into()),
            bull_agent_model: Some("gpt-5".into()),
            bear_agent_model: Some("gpt-5-mini".into()),
            balanced_agent_model: Some("claude-sonnet".into()),
            openai_api_key: Some("sk-openai".into()),
            anthropic_api_key: Some("sk-anthropic".into()),
            fmp_api_key: Some("fmp-key".into()),
            fred_api_key: Some("fred-key".into()),
            tavily_api_key: Some("tavily-key".into()),
            // Local-suite fields are not part of the cloud gate, so the all-green
            // cloud baseline leaves them unset.
            ..AppConfig::default()
        }
    }

    fn category(report: &ValidationReport, kind: WarningKind) -> Option<&WarningCategory> {
        report.categories.iter().find(|c| c.kind == kind)
    }

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        storage::init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn load_prefers_a_saved_value_over_the_environment() {
        // A saved value is authoritative regardless of any ambient env var, so
        // these assertions are env-independent. The env-fallback direction is
        // `Option::or` and is left to `from_env` + the live smoke to exercise,
        // to keep this test free of (unsafe, racy) env mutation.
        let conn = mem();
        storage::set_setting(&conn, KEY_MAIN_AGENT_MODEL, "claude-sonnet").unwrap();
        storage::set_setting(&conn, KEY_OPENAI_API_KEY, "sk-saved").unwrap();
        let cfg = AppConfig::load(&conn);
        assert_eq!(cfg.main_agent_model.as_deref(), Some("claude-sonnet"));
        assert_eq!(cfg.openai_api_key.as_deref(), Some("sk-saved"));
    }

    #[test]
    fn load_reads_a_blank_saved_value_that_validate_treats_as_unset() {
        // A model explicitly cleared in Settings is stored as "" and read back as
        // Some(""), which `present` (and thus `validate`) reports as unselected.
        let conn = mem();
        storage::set_setting(&conn, KEY_MAIN_AGENT_MODEL, "").unwrap();
        let cfg = AppConfig::load(&conn);
        assert_eq!(cfg.main_agent_model.as_deref(), Some(""));
        assert!(present(&cfg.main_agent_model).is_none());
    }

    #[test]
    fn load_reads_local_suite_settings_and_validate_ignores_them() {
        // The local-suite settings round-trip through the same K/V store...
        let conn = mem();
        storage::set_setting(&conn, KEY_LOCAL_DAEMON_ENDPOINT, "http://localhost:11434").unwrap();
        storage::set_setting(&conn, KEY_LOCAL_REASONER_MODEL, "qwen3.5:122b").unwrap();
        let cfg = AppConfig::load(&conn);
        assert_eq!(
            cfg.local_daemon_endpoint.as_deref(),
            Some("http://localhost:11434")
        );
        assert_eq!(cfg.local_reasoner_model.as_deref(), Some("qwen3.5:122b"));
        // ...but they are not part of the cloud gate: with only local keys set, the
        // cloud gate still blocks on its own (agents/tokens/creds) and never emits a
        // LocalModels category — that lives in `local_model::local_gate`.
        let report = validate(&cfg);
        assert!(report.is_blocked);
        assert!(category(&report, WarningKind::AgentConfiguration).is_some());
        assert!(!report
            .categories
            .iter()
            .any(|c| c.kind == WarningKind::LocalModels));
    }

    #[test]
    fn complete_config_is_not_blocked_and_has_no_categories() {
        let report = validate(&complete());
        assert!(!report.is_blocked);
        assert!(report.categories.is_empty());
    }

    #[test]
    fn missing_one_agent_model_blocks_with_agent_category() {
        let mut cfg = complete();
        cfg.bear_agent_model = None;
        let report = validate(&cfg);
        assert!(report.is_blocked);
        let cat = category(&report, WarningKind::AgentConfiguration).expect("agent category");
        assert_eq!(cat.items.len(), 1);
        assert!(cat.items[0].contains("Bear Analyst"), "{:?}", cat.items);
        assert!(
            cat.items[0].contains("No model selected"),
            "{:?}",
            cat.items
        );
    }

    #[test]
    fn unknown_agent_model_label_is_an_agent_category_item() {
        let mut cfg = complete();
        cfg.bull_agent_model = Some("bogus".into());
        let report = validate(&cfg);
        assert!(report.is_blocked);
        let cat = category(&report, WarningKind::AgentConfiguration).expect("agent category");
        assert!(cat.items[0].contains("Bull Analyst"), "{:?}", cat.items);
        assert!(cat.items[0].contains("bogus"), "{:?}", cat.items);
    }

    #[test]
    fn blank_value_is_treated_as_missing() {
        let mut cfg = complete();
        cfg.main_agent_model = Some("   ".into());
        let report = validate(&cfg);
        let cat = category(&report, WarningKind::AgentConfiguration).expect("agent category");
        assert!(cat.items[0].contains("Main Agent"), "{:?}", cat.items);
        assert!(
            cat.items[0].contains("No model selected"),
            "{:?}",
            cat.items
        );
    }

    #[test]
    fn missing_token_blocks_with_token_category() {
        let mut cfg = complete();
        cfg.openai_api_key = None;
        let report = validate(&cfg);
        assert!(report.is_blocked);
        let cat = category(&report, WarningKind::ApiTokens).expect("token category");
        assert_eq!(cat.items.len(), 1);
        assert!(cat.items[0].contains("OpenAI"), "{:?}", cat.items);
    }

    #[test]
    fn missing_provider_credential_blocks_with_credential_category() {
        let mut cfg = complete();
        cfg.fmp_api_key = None;
        let report = validate(&cfg);
        assert!(report.is_blocked);
        let cat = category(&report, WarningKind::ProviderCredentials).expect("credential category");
        assert_eq!(cat.items.len(), 1);
        assert!(
            cat.items[0].contains("Financial Modeling Prep"),
            "{:?}",
            cat.items
        );
    }

    #[test]
    fn missing_fred_credential_blocks_with_credential_category() {
        let mut cfg = complete();
        cfg.fred_api_key = None;
        let report = validate(&cfg);
        assert!(report.is_blocked);
        let cat = category(&report, WarningKind::ProviderCredentials).expect("credential category");
        assert_eq!(cat.items.len(), 1);
        assert!(cat.items[0].contains("FRED"), "{:?}", cat.items);
    }

    #[test]
    fn multiple_gaps_produce_multiple_categories() {
        let cfg = AppConfig::default(); // everything missing
        let report = validate(&cfg);
        assert!(report.is_blocked);
        assert!(category(&report, WarningKind::AgentConfiguration).is_some());
        assert!(category(&report, WarningKind::ApiTokens).is_some());
        assert!(category(&report, WarningKind::ProviderCredentials).is_some());
        // Each category condenses to a single line that names every missing item.
        let agents = category(&report, WarningKind::AgentConfiguration).unwrap();
        assert_eq!(agents.items.len(), 1);
        for name in [
            "Main Agent",
            "Bull Analyst",
            "Bear Analyst",
            "Balanced Analyst",
        ] {
            assert!(agents.items[0].contains(name), "{:?}", agents.items);
        }
        let tokens = category(&report, WarningKind::ApiTokens).unwrap();
        assert_eq!(tokens.items.len(), 1);
        assert!(tokens.items[0].contains("OpenAI") && tokens.items[0].contains("Anthropic"));
        let creds = category(&report, WarningKind::ProviderCredentials).unwrap();
        assert_eq!(creds.items.len(), 1);
        assert!(
            creds.items[0].contains("Financial Modeling Prep")
                && creds.items[0].contains("FRED")
                && creds.items[0].contains("Tavily")
        );
    }

    #[test]
    fn blocked_summary_lists_the_blocking_titles() {
        let report = validate(&AppConfig::default());
        let summary = blocked_summary(&report);
        assert!(summary.contains("Agent configuration"), "{summary}");
        assert!(summary.contains("API tokens"), "{summary}");
        assert!(summary.contains("Provider credentials"), "{summary}");
    }

    #[test]
    fn main_agent_config_resolves_model_and_matching_provider_key() {
        let cfg = complete(); // main = claude-opus -> Anthropic
        let mac = cfg.main_agent_config().expect("resolves");
        assert_eq!(mac.model, AgentModel::ClaudeOpus);
        assert_eq!(mac.api_key, "sk-anthropic");
    }

    #[test]
    fn main_agent_config_errors_when_provider_key_missing() {
        let mut cfg = complete();
        cfg.main_agent_model = Some("gpt-5".into()); // -> OpenAI
        cfg.openai_api_key = None;
        // Match rather than `unwrap_err` so `MainAgentConfig` (which carries the
        // API key) never needs a `Debug` impl that could print the secret.
        let err = match cfg.main_agent_config() {
            Ok(_) => panic!("expected an error when the provider key is missing"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("OPENAI_API_KEY"), "{err}");
    }

    #[test]
    fn fmp_key_resolves_present_value_and_errors_when_missing() {
        let cfg = complete();
        assert_eq!(cfg.fmp_key().unwrap(), "fmp-key");

        let mut blank = complete();
        blank.fmp_api_key = Some("   ".into()); // present-but-blank reads as unset
        let err = blank.fmp_key().unwrap_err();
        assert!(err.to_string().contains("FMP_API_KEY"), "{err}");
    }

    #[test]
    fn fred_key_resolves_present_value_and_errors_when_missing() {
        let cfg = complete();
        assert_eq!(cfg.fred_key().unwrap(), "fred-key");

        let mut blank = complete();
        blank.fred_api_key = Some("   ".into()); // present-but-blank reads as unset
        let err = blank.fred_key().unwrap_err();
        assert!(err.to_string().contains("FRED_API_KEY"), "{err}");
    }

    #[test]
    fn tavily_key_resolves_present_value_and_errors_when_missing() {
        let cfg = complete();
        assert_eq!(cfg.tavily_key().unwrap(), "tavily-key");

        let mut blank = complete();
        blank.tavily_api_key = Some("   ".into()); // present-but-blank reads as unset
        let err = blank.tavily_key().unwrap_err();
        assert!(err.to_string().contains("TAVILY_API_KEY"), "{err}");
    }

    #[test]
    fn openai_key_resolves_present_value_and_errors_when_missing() {
        let cfg = complete();
        assert_eq!(cfg.openai_key().unwrap(), "sk-openai");

        let mut blank = complete();
        blank.openai_api_key = Some("   ".into()); // present-but-blank reads as unset
        let err = blank.openai_key().unwrap_err();
        assert!(err.to_string().contains("OPENAI_API_KEY"), "{err}");
    }

    #[test]
    fn anthropic_key_resolves_present_value_and_errors_when_missing() {
        let cfg = complete();
        assert_eq!(cfg.anthropic_key().unwrap(), "sk-anthropic");

        let mut blank = complete();
        blank.anthropic_api_key = Some("   ".into()); // present-but-blank reads as unset
        let err = blank.anthropic_key().unwrap_err();
        assert!(err.to_string().contains("ANTHROPIC_API_KEY"), "{err}");
    }

    #[test]
    fn job_warning_kinds_are_non_blocking() {
        assert!(!WarningKind::FailedJob.is_blocking());
        assert!(WarningKind::AgentConfiguration.is_blocking());
    }

    #[test]
    fn schwab_warning_blocks_but_never_enters_the_cloud_validate_gate() {
        // Schwab gates the local jobs, so it blocks like the other config gaps...
        assert!(WarningKind::Schwab.is_blocking());
        // ...but it is a local-suite category produced where the Keychain rail is
        // reachable, never by the cloud-report gate. `validate` is pure over `AppConfig`
        // (no token rail), so it must never emit a Schwab category — neither for a
        // fully-configured cloud config nor a fully-empty one — which is what keeps a
        // disconnected Schwab account from ever blocking the Market Signal Report.
        for cfg in [complete(), AppConfig::default()] {
            let report = validate(&cfg);
            assert!(!report
                .categories
                .iter()
                .any(|c| c.kind == WarningKind::Schwab));
        }
    }
}
