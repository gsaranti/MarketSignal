//! The pre-run execution gate: structured configuration validation.
//!
//! `docs/weekly-report-workflow.md §Step 1` and `docs/configuration.md` require
//! that no report run starts until the configuration is complete — all four
//! agent models selected, both provider API tokens present, and both external
//! data-provider credentials present. This module turns that requirement into a
//! pure function: `validate(&AppConfig) -> ValidationReport`. The report drives
//! both the backend block (the Tauri command refuses to run when blocked) and
//! the frontend Persistent Warning Area (`docs/interface.md`).
//!
//! `AppConfig` is the interim config substrate: it reads from environment
//! variables (the same surface `model_agent::ModelMainAgent` already used),
//! standing in for the Settings store that lands later. The env→store swap is
//! confined to `AppConfig::from_env`; `validate` is pure over the struct and
//! never touches the environment, so the pass/block matrix is unit-testable
//! without env mutation.
//!
//! Scope note: network reachability — the gate's fourth Step-1 check — and the
//! two scheduler-owned warning categories (failed / missed jobs) are modeled in
//! `WarningKind` but not produced here. Network unreachability surfaces as a
//! job *failure* (`docs/scheduling.md §Offline Behavior`), which lands with the
//! scheduler slice that owns the failed-job category.

use anyhow::{anyhow, Result};
use serde::Serialize;

use crate::model_agent::{AgentModel, MainAgentConfig, Provider};

/// The five de-duplicating Persistent Warning Area categories (walk Q4,
/// `docs/interface.md §Persistent Warning Area`). The three configuration
/// categories are produced by `validate`; the two job categories are produced by
/// the scheduler and are modeled here so the warning structure is whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningKind {
    AgentConfiguration,
    ApiTokens,
    ProviderCredentials,
    FailedJob,
    MissedScheduledJob,
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
        )
    }
}

/// One warning category for display: its kind, a human-readable title used as
/// the Persistent Warning Area row label, and the concrete missing items.
#[derive(Debug, Clone, Serialize)]
pub struct WarningCategory {
    pub kind: WarningKind,
    pub title: String,
    pub items: Vec<String>,
}

/// The result of validating the configuration: the active warning categories
/// (only non-empty ones are included) and whether any of them blocks a run.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub categories: Vec<WarningCategory>,
    pub is_blocked: bool,
}

/// The interim configuration substrate, read from the environment. Each field is
/// `None` when its variable is unset; blank values are treated as unset by
/// `present`. Replaced by the Settings store later — only `from_env` changes.
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    pub main_agent_model: Option<String>,
    pub bull_agent_model: Option<String>,
    pub bear_agent_model: Option<String>,
    pub balanced_agent_model: Option<String>,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub fmp_api_key: Option<String>,
    pub tavily_api_key: Option<String>,
}

/// A set-and-non-blank value, or `None`. An env var set to "" is effectively
/// unset for gate purposes, so it must not pass validation.
fn present(opt: &Option<String>) -> Option<&str> {
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
            tavily_api_key: get("TAVILY_API_KEY"),
        }
    }

    /// Resolve the Main Agent's adapter config from validated configuration:
    /// the selected model and the API key for that model's provider. Used by the
    /// command once the gate has passed, and by the live smoke via
    /// `ModelMainAgent::from_env`. Errors mirror the gate's wording so a caller
    /// that bypasses the gate still gets a legible message.
    pub fn main_agent_config(&self) -> Result<MainAgentConfig> {
        let label = present(&self.main_agent_model)
            .ok_or_else(|| anyhow!("MARKET_SIGNAL_MAIN_AGENT_MODEL is not set (no Main Agent model selected)"))?;
        let model = AgentModel::from_config_label(label)?;
        let (key_opt, var) = match model.provider() {
            Provider::OpenAi => (&self.openai_api_key, "OPENAI_API_KEY"),
            Provider::Anthropic => (&self.anthropic_api_key, "ANTHROPIC_API_KEY"),
        };
        let api_key = present(key_opt)
            .ok_or_else(|| anyhow!("{var} is not set (required for the selected Main Agent model)"))?
            .to_string();
        Ok(MainAgentConfig { model, api_key })
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
pub fn validate(cfg: &AppConfig) -> ValidationReport {
    let mut categories: Vec<WarningCategory> = Vec::new();

    // Agent configuration: all four agents need a present, parseable model.
    let slots = [
        AgentSlot { name: "Main Agent", value: &cfg.main_agent_model },
        AgentSlot { name: "Bull Analyst", value: &cfg.bull_agent_model },
        AgentSlot { name: "Bear Analyst", value: &cfg.bear_agent_model },
        AgentSlot { name: "Balanced Analyst", value: &cfg.balanced_agent_model },
    ];
    let mut agent_items = Vec::new();
    for slot in slots {
        match present(slot.value) {
            None => agent_items.push(format!("{} — no model selected", slot.name)),
            Some(label) => {
                if AgentModel::from_config_label(label).is_err() {
                    agent_items.push(format!("{} — unknown model \"{label}\"", slot.name));
                }
            }
        }
    }
    if !agent_items.is_empty() {
        categories.push(WarningCategory {
            kind: WarningKind::AgentConfiguration,
            title: "Agent configuration".to_string(),
            items: agent_items,
        });
    }

    // API tokens: both are always required (the fixed internal stages span both
    // providers — docs/configuration.md §API Tokens).
    let mut token_items = Vec::new();
    if present(&cfg.openai_api_key).is_none() {
        token_items.push("OpenAI — API token missing".to_string());
    }
    if present(&cfg.anthropic_api_key).is_none() {
        token_items.push("Anthropic — API token missing".to_string());
    }
    if !token_items.is_empty() {
        categories.push(WarningCategory {
            kind: WarningKind::ApiTokens,
            title: "API tokens".to_string(),
            items: token_items,
        });
    }

    // External data-provider credentials: FMP and Tavily are both required to
    // run (docs/configuration.md §External Data Provider Credentials).
    let mut cred_items = Vec::new();
    if present(&cfg.fmp_api_key).is_none() {
        cred_items.push("Financial Modeling Prep — credential missing".to_string());
    }
    if present(&cfg.tavily_api_key).is_none() {
        cred_items.push("Tavily — credential missing".to_string());
    }
    if !cred_items.is_empty() {
        categories.push(WarningCategory {
            kind: WarningKind::ProviderCredentials,
            title: "Provider credentials".to_string(),
            items: cred_items,
        });
    }

    let is_blocked = categories.iter().any(|c| c.kind.is_blocking());
    ValidationReport { categories, is_blocked }
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

/// The scheduler's pre-run decision for a *scheduled* fire. Distinct from a
/// manual run, which ignores the enable flag — disabling the weekly schedule
/// must not block a manual "Generate". No `Debug` derive: the `Proceed` variant
/// carries `MainAgentConfig`, which deliberately has no `Debug` so an API key
/// can never be printed.
pub enum ScheduledRun {
    /// Gate passed: carries the resolved Main Agent adapter config to run with.
    Proceed(MainAgentConfig),
    /// The weekly job is disabled — an expected, quiet no-op (no diagnostic).
    Disabled,
    /// Blocked by a noteworthy reason (incomplete config or an unresolved model
    /// key) worth logging; the human-readable reason rides along.
    Blocked(String),
}

/// Decide whether a scheduled fire should proceed: the enable flag, then the
/// execution gate, then a resolvable Main Agent model + key (`docs/weekly-report
/// -workflow.md §Step 1`). Pure over its inputs — `validate` and
/// `main_agent_config` read only from `cfg` — so the enabled / blocked / proceed
/// composition the scheduler walks is unit-testable without the environment or a
/// running app. The `main_agent_config` error arm is defensive: after a passing
/// `validate` (which already requires both provider keys and a parseable main
/// model) it is effectively unreachable, mirroring the manual command's pattern.
pub fn decide_scheduled_run(cfg: &AppConfig, enabled: bool) -> ScheduledRun {
    if !enabled {
        return ScheduledRun::Disabled;
    }
    if validate(cfg).is_blocked {
        return ScheduledRun::Blocked("configuration incomplete — run skipped".to_string());
    }
    match cfg.main_agent_config() {
        Ok(main_config) => ScheduledRun::Proceed(main_config),
        Err(e) => ScheduledRun::Blocked(e.to_string()),
    }
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
            tavily_api_key: Some("tavily-key".into()),
        }
    }

    fn category(report: &ValidationReport, kind: WarningKind) -> Option<&WarningCategory> {
        report.categories.iter().find(|c| c.kind == kind)
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
        assert!(cat.items[0].contains("no model selected"), "{:?}", cat.items);
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
        assert!(cat.items[0].contains("no model selected"), "{:?}", cat.items);
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
        assert!(cat.items[0].contains("Financial Modeling Prep"), "{:?}", cat.items);
    }

    #[test]
    fn multiple_gaps_produce_multiple_categories() {
        let cfg = AppConfig::default(); // everything missing
        let report = validate(&cfg);
        assert!(report.is_blocked);
        assert!(category(&report, WarningKind::AgentConfiguration).is_some());
        assert!(category(&report, WarningKind::ApiTokens).is_some());
        assert!(category(&report, WarningKind::ProviderCredentials).is_some());
        // All four agents and both tokens and both creds reported.
        let agents = category(&report, WarningKind::AgentConfiguration).unwrap();
        assert_eq!(agents.items.len(), 4);
        let tokens = category(&report, WarningKind::ApiTokens).unwrap();
        assert_eq!(tokens.items.len(), 2);
        let creds = category(&report, WarningKind::ProviderCredentials).unwrap();
        assert_eq!(creds.items.len(), 2);
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
    fn job_warning_kinds_are_non_blocking() {
        assert!(!WarningKind::FailedJob.is_blocking());
        assert!(!WarningKind::MissedScheduledJob.is_blocking());
        assert!(WarningKind::AgentConfiguration.is_blocking());
    }

    #[test]
    fn scheduled_run_proceeds_for_a_complete_enabled_config() {
        // Match rather than unwrap so `MainAgentConfig` never needs a `Debug`
        // impl that could print the secret.
        match decide_scheduled_run(&complete(), true) {
            ScheduledRun::Proceed(mac) => assert_eq!(mac.model, AgentModel::ClaudeOpus),
            _ => panic!("expected Proceed for a complete, enabled config"),
        }
    }

    #[test]
    fn scheduled_run_is_a_quiet_skip_when_disabled() {
        // A complete config that is disabled must not run — and silently, so a
        // disabled weekly schedule produces no per-window diagnostic.
        assert!(matches!(
            decide_scheduled_run(&complete(), false),
            ScheduledRun::Disabled
        ));
    }

    #[test]
    fn scheduled_run_is_blocked_when_config_incomplete() {
        let mut cfg = complete();
        cfg.tavily_api_key = None; // a blocking gap, even with the job enabled
        match decide_scheduled_run(&cfg, true) {
            ScheduledRun::Blocked(reason) => assert!(reason.contains("incomplete"), "{reason}"),
            _ => panic!("expected Blocked when a required credential is missing"),
        }
    }
}
