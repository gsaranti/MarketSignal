//! Step-6d distillation — the deterministic single-vs-hierarchical
//! consolidation primitive (`docs/portfolio-workflow.md` §Step 6d;
//! `docs/web-research.md §The research loop and context management`).
//!
//! The reasoner in non-thinking mode consolidates each topic's **complete**
//! findings into the compact object interpretation reads — never a
//! re-distillation of already-distilled notes. The **orchestrator** — not the
//! model — chooses the shape deterministically by the consolidation input's
//! full size (every topic's findings, its evidence-ledger claims, and the
//! merged per-topic priors) against the call's input budget: a **single
//! pass** when it fits, else **hierarchical** (a tier-1 call per topic-tree,
//! then one reduce), with the pass-seam sub-distillation fallback for a topic
//! whose own complete input would overflow one call and a cap past which the
//! lowest-priority whole passes fail-soft to a recorded gap.
//!
//! Portfolio's cross-run reuse merges **per topic** where that topic is first
//! reduced — fresh supersedes cached, newest wins — and the reduce applies the
//! same rule *globally*, emitting **both** artifacts from one reconciliation:
//! the combined object interpretation reads and the **reconciled per-topic
//! seed layer** that persists as the next run's seeds (the raw tier-1 output
//! is never itself persisted). Claim vintages and cached-vs-fresh provenance
//! are **app-resolved by source URL** against this run's evidence ledger and
//! the prior layer — the model never stamps a vintage.
//!
//! The typed fields (`research_forward_assumption`,
//! `validated_leading_indicator`, `forensic_event`,
//! `pre_profit_execution_observations` + the backfill record) exist only
//! where their consumers do: a `role_risk_only` holding's distillation is
//! pure consolidation and emits none.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::portfolio::pre_profit::{BackfillAttempt, PreProfitObservation};
use crate::portfolio::research::{DistilledClaim, HoldingResearch, TopicDistillate};

// ---------------------------------------------------------------------------
// Constants (drafted, calibratable — `docs/configuration.md §Research Context
// Management`: generous, conservative defaults)
// ---------------------------------------------------------------------------

/// Fraction of a consolidation call's input budget above which the
/// orchestrator switches from a single pass to hierarchical (headroom left
/// for the instruction scaffolding and the structured output).
pub const OVERFLOW_THRESHOLD: f64 = 0.6;

/// Rough chars-per-token for sizing a call's input budget off its `num_ctx`.
pub const CHARS_PER_TOKEN: f64 = 3.0;

/// The per-holding cap on pass-level sub-distillations; beyond it the
/// lowest-priority whole passes drop to a recorded gap rather than overrun.
pub const SUB_DISTILLATION_CAP: usize = 4;

/// The input budget for one consolidation call, derived from the resolved
/// distill `num_ctx`.
pub fn input_budget_chars(num_ctx: u32) -> usize {
    (f64::from(num_ctx) * CHARS_PER_TOKEN * OVERFLOW_THRESHOLD) as usize
}

// ---------------------------------------------------------------------------
// Output shapes
// ---------------------------------------------------------------------------

/// The typed forward assumption — the only thing that can reach the engine's
/// Step-6e target refinement (`docs/portfolio-workflow.md` §Step 6d Returns).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchForwardAssumption {
    /// What kind of fact this is (issued guidance, signed contract, filed
    /// figure, commodity/ASP turn …).
    pub fact_type: String,
    pub numeric_value: f64,
    /// The stated range endpoints where the source gives a range rather than a
    /// point (guidance "between X and Y"): both must appear in the cited page
    /// and bound `numeric_value`, so a range fact stays corroboratable without
    /// the midpoint itself having to appear verbatim.
    #[serde(default)]
    pub stated_low: Option<f64>,
    #[serde(default)]
    pub stated_high: Option<f64>,
    pub units: String,
    /// The fact's period / as-of date.
    pub as_of: String,
    pub source_url: String,
    /// Extraction confidence, 0–1.
    pub confidence: f64,
    /// The target assumption it affects (which driver / scenario input).
    pub affects: String,
    /// The typed two-value declaration — a claim the engine validates under
    /// the app-owned Step-6e conflict policy, never a rule the model selects.
    pub conflict_handling: ConflictHandling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictHandling {
    Supplement,
    Supersede,
}

/// The typed validated leading indicator — ledger-driver evidence (its
/// conviction-raise citation role is retired suite-wide with `portfolio-v7`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatedLeadingIndicator {
    pub metric_name: String,
    pub value: f64,
    pub direction: IndicatorDirection,
    pub as_of: String,
    pub source_url: String,
    pub confidence: f64,
    /// The thesis-ledger key driver it confirms (prose — model-attributed
    /// context; the id below carries the referential claim).
    pub confirms_driver: String,
    /// The cited ledger driver's app-assigned `driver_id` (ruled 2026-08-24):
    /// the model picks it from the ids rendered in the prompt.
    #[serde(default)]
    pub confirms_driver_id: String,
    /// **App-computed at validation, never model-set** (absent from the
    /// schema; any model-emitted value is overwritten): whether
    /// `confirms_driver_id` resolves to a current ledger driver. Only a
    /// verified reference lets the indicator suppress the narrative cap —
    /// an unverified one stays visible evidence.
    #[serde(default)]
    pub driver_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IndicatorDirection {
    InflectingUp,
    InflectingDown,
}

/// The research-fed forensic claim — the fraud kind's sole producer
/// (`docs/portfolio-workflow.md` §Step 6d Returns; the producer contract at
/// trade-opportunities-workflow.md §Step 5c). App-validated, and **advisory by
/// the 2026-08-24 ruling**: it rides the audit and the interpretation prompt
/// as cited attention evidence — the hard rule trips from the item-classified
/// filing kinds alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForensicEventClaim {
    /// Only `fraud` validates (restatement / auditor-change are
    /// filings-classified, never research-fed).
    pub kind: String,
    pub issuer: String,
    pub event_date: String,
    pub source_url: String,
    pub confidence: f64,
}

/// How the distillation ran — logged to the audit so the fan-out is never
/// silent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DistillShape {
    SinglePass,
    Hierarchical {
        tier1_calls: usize,
        subdistilled_topics: usize,
        dropped_passes: usize,
    },
}

/// The two mutually consistent artifacts (plus the typed side-channels) one
/// reconciliation emits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistilledResearch {
    /// The compact combined object interpretation reads.
    pub combined: String,
    /// The reconciled per-topic seed layer — persists as the next run's seeds.
    pub topic_layer: Vec<TopicDistillate>,
    /// Analyzed or dormant topics the model failed to re-emit — their stored
    /// seed rows are deleted rather than left stale (each is also a gap line).
    #[serde(default)]
    pub unreconciled_topics: Vec<String>,
    pub forward_assumption: Option<ResearchForwardAssumption>,
    pub leading_indicator: Option<ValidatedLeadingIndicator>,
    pub forensic_event: Option<ForensicEventClaim>,
    pub pre_profit_observations: Vec<PreProfitObservation>,
    pub backfill: Option<BackfillAttempt>,
    pub shape: DistillShape,
    /// Recorded degraded-input gaps (dropped claims/fields, dropped passes).
    pub gaps: Vec<String>,
}

/// The per-holding research audit record (`docs/storage.md §Local Analysis
/// Suite Storage` — the research-derived artifacts): source URLs with their
/// retrieval timestamps, the distilled findings (the combined object and the
/// reconciled per-topic layer), the per-topic seeded-vs-cold decisions, the
/// budget spend, the degraded gaps, the distillation shape, and the typed
/// side-channels as validated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchAuditRecord {
    pub combined: String,
    pub seed_layer: Vec<TopicDistillate>,
    pub shape: DistillShape,
    pub fetches_spent: u32,
    pub elapsed_secs: u64,
    pub tavily_fallback_used: bool,
    pub seed_decisions: Vec<String>,
    /// "url (retrieved_at)" lines from the evidence ledger.
    pub sources: Vec<String>,
    pub gaps: Vec<String>,
    /// Topics whose stored seed rows the job deleted because the distillation
    /// failed to re-emit them reconciled (mirrored from the distilled layer).
    #[serde(default)]
    pub unreconciled_topics: Vec<String>,
    #[serde(default)]
    pub forward_assumption: Option<ResearchForwardAssumption>,
    #[serde(default)]
    pub leading_indicator: Option<ValidatedLeadingIndicator>,
    #[serde(default)]
    pub forensic_event: Option<ForensicEventClaim>,
    /// The Step-6e conflict-policy resolution for the forward assumption —
    /// the rule the engine matched, or the failed condition that rejected it
    /// (`docs/portfolio-workflow.md` §Step 6e; every resolution is recorded).
    #[serde(default)]
    pub forward_assumption_resolution: Option<String>,
}

/// Deterministic offline consolidation — the defaulted trait path for stub
/// analysts and the demo: joins the passes' findings without a model call,
/// builds the per-topic layer straight from the fresh evidence ledger, and
/// emits no typed field. Pipeline-shaped, model-free.
pub fn offline_consolidate(inputs: &DistillInputs<'_>) -> DistilledResearch {
    let mut combined = String::new();
    let mut topic_layer = Vec::new();
    for topic in &inputs.research.topics {
        if topic.passes.is_empty() {
            continue;
        }
        let mut claims = Vec::new();
        for pass in &topic.passes {
            if !combined.is_empty() {
                combined.push(' ');
            }
            combined.push_str(&pass.findings);
            for c in &pass.claims {
                claims.push(DistilledClaim {
                    claim: c.claim.clone(),
                    source_url: c.source_url.clone(),
                    vintage: c.retrieved_at.clone(),
                    cached: false,
                    related_condition_id: None,
                });
            }
        }
        topic_layer.push(TopicDistillate {
            topic_key: topic.topic_key.clone(),
            vintage: inputs.now.to_rfc3339(),
            summary: topic
                .passes
                .first()
                .map(|p| p.findings.clone())
                .unwrap_or_default(),
            claims,
        });
    }
    if combined.is_empty() {
        combined = "No research findings.".to_string();
    }
    DistilledResearch {
        combined,
        topic_layer,
        unreconciled_topics: Vec::new(),
        forward_assumption: None,
        leading_indicator: None,
        forensic_event: None,
        pre_profit_observations: Vec::new(),
        backfill: None,
        shape: DistillShape::SinglePass,
        gaps: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Seam + inputs
// ---------------------------------------------------------------------------

/// The model seam: one non-thinking, schema-constrained consolidation call.
/// The live implementation wraps the resident reasoner (or the fast tier);
/// tests script it.
pub trait DistillModel {
    fn distill_call(&self, stage: &str, prompt: String, schema: &Value) -> Result<String>;
}

/// Everything one holding's distillation needs.
pub struct DistillInputs<'a> {
    pub symbol: &'a str,
    /// The issuer's name where the dossier resolved one — the forensic claim's
    /// issuer-identity validation reads it beside the symbol.
    pub company_name: Option<&'a str>,
    pub research: &'a HoldingResearch,
    /// The prior per-topic layer, already filtered to non-expired topic
    /// objects (the seed gate) — merged per topic at its first reduction.
    pub priors: &'a [TopicDistillate],
    /// The prior ledger's conditions with their app-assigned ids — rendered into
    /// every claim-emitting prompt for citation and the referential surface
    /// `related_condition_id` validates against (the `confirms_driver_id`
    /// pattern; `docs/portfolio-workflow.md` §Step 6d).
    pub ledger_conditions: &'a [crate::portfolio::LedgerCondition],
    /// The prior ledger's key drivers with their app-assigned ids — rendered
    /// into the prompt and the referential surface `confirms_driver_id`
    /// verifies against (ruled 2026-08-24).
    pub ledger_key_drivers: &'a [crate::portfolio::KeyDriver],
    /// Pure consolidation: no typed fields on this branch.
    pub role_risk: bool,
    /// Whether pre-profit observation rows may be emitted.
    pub overlay_eligible: bool,
    pub input_budget_chars: usize,
    /// This run's timestamp — the new layer's topic vintage.
    pub now: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

fn claim_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "claim": { "type": "string" },
            "source_url": { "type": "string" },
            "related_condition_id": { "type": ["string", "null"] }
        },
        "required": ["claim", "source_url"]
    })
}

fn topic_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "topic_key": { "type": "string" },
            "summary": { "type": "string" },
            "claims": { "type": "array", "items": claim_schema() }
        },
        "required": ["topic_key", "summary", "claims"]
    })
}

/// The tier-1 (and pass-level sub-distillation) schema: one topic's portion.
fn tier1_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "claims": { "type": "array", "items": claim_schema() }
        },
        "required": ["summary", "claims"]
    })
}

/// The reduce / single-pass schema. The typed side-channels ride only the
/// priced branch (`role_risk` gets the reduced shape).
fn combined_schema(role_risk: bool, overlay_eligible: bool) -> Value {
    let mut properties = json!({
        "combined_findings": { "type": "string" },
        "topics": { "type": "array", "items": topic_schema() }
    });
    let mut required = vec!["combined_findings", "topics"];
    if !role_risk {
        properties["forward_assumption"] = json!({
            "type": ["object", "null"],
            "properties": {
                "fact_type": { "type": "string" },
                "numeric_value": { "type": "number" },
                "stated_low": { "type": ["number", "null"] },
                "stated_high": { "type": ["number", "null"] },
                "units": { "type": "string" },
                "as_of": { "type": "string" },
                "source_url": { "type": "string" },
                "confidence": { "type": "number" },
                "affects": { "type": "string" },
                "conflict_handling": { "type": "string", "enum": ["supplement", "supersede"] }
            },
            "required": ["fact_type", "numeric_value", "stated_low", "stated_high", "units",
                          "as_of", "source_url", "confidence", "affects", "conflict_handling"]
        });
        properties["leading_indicator"] = json!({
            "type": ["object", "null"],
            "properties": {
                "metric_name": { "type": "string" },
                "value": { "type": "number" },
                "direction": { "type": "string", "enum": ["inflecting-up", "inflecting-down"] },
                "as_of": { "type": "string" },
                "source_url": { "type": "string" },
                "confidence": { "type": "number" },
                "confirms_driver": { "type": "string" },
                // The cited ledger driver's app-assigned id, from the prompt's
                // rendered list (`driver_verified` is app-computed and
                // deliberately NOT in this schema).
                "confirms_driver_id": { "type": "string" }
            },
            "required": ["metric_name", "value", "direction", "as_of", "source_url",
                          "confidence", "confirms_driver", "confirms_driver_id"]
        });
        properties["forensic_event"] = json!({
            "type": ["object", "null"],
            "properties": {
                // Research feeds ONLY the fraud kind — restatement /
                // auditor-change are filings-classified (the schema teaches
                // the contract; validation still enforces it).
                "kind": { "type": "string", "enum": ["fraud"] },
                "issuer": { "type": "string" },
                "event_date": { "type": "string" },
                "source_url": { "type": "string" },
                "confidence": { "type": "number" }
            },
            "required": ["kind", "issuer", "event_date", "source_url", "confidence"]
        });
        required.extend(["forward_assumption", "leading_indicator", "forensic_event"]);
        if overlay_eligible {
            properties["pre_profit_observations"] = json!({
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "metric_kind": { "type": "string",
                            "enum": ["production", "deliveries", "bookings", "backlog",
                                     "reservations", "unit-economics"] },
                        "observation_role": { "type": "string",
                            "enum": ["actual", "guidance-low", "guidance-high",
                                     "point-guidance", "contextual-level"] },
                        "polarity": { "type": "string",
                            "enum": ["higher-is-better", "lower-is-better", "target-band"] },
                        "numeric_value": { "type": "number" },
                        "units": { "type": "string" },
                        "period": { "type": "string" },
                        "issuer_scope": { "type": "string" },
                        "source_url": { "type": "string" },
                        "published_at": { "type": "string" },
                        "confidence": { "type": "number" }
                    },
                    "required": ["metric_kind", "observation_role", "polarity", "numeric_value",
                                  "units", "period", "issuer_scope", "source_url",
                                  "published_at", "confidence"]
                }
            });
            properties["backfill"] = json!({
                "type": ["object", "null"],
                "properties": {
                    "metric_kind": { "type": "string",
                        "enum": ["production", "deliveries", "bookings", "backlog",
                                 "reservations", "unit-economics"] },
                    "units": { "type": "string" },
                    "issuer_scope": { "type": "string" },
                    "checked_periods": { "type": "array", "items": { "type": "string" } },
                    "sources": { "type": "array", "items": { "type": "string" } },
                    "coverage": { "type": "string", "enum": ["complete", "partial", "unscorable"] }
                },
                "required": ["metric_kind", "units", "issuer_scope", "checked_periods",
                              "sources", "coverage"]
            });
            required.extend(["pre_profit_observations", "backfill"]);
        }
    }
    json!({ "type": "object", "properties": properties, "required": required })
}

// ---------------------------------------------------------------------------
// Wire shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ClaimWire {
    #[serde(default)]
    claim: String,
    #[serde(default)]
    source_url: String,
    #[serde(default)]
    related_condition_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TopicWire {
    #[serde(default)]
    topic_key: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    claims: Vec<ClaimWire>,
}

#[derive(Debug, Deserialize)]
struct Tier1Wire {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    claims: Vec<ClaimWire>,
}

#[derive(Debug, Deserialize)]
struct CombinedWire {
    #[serde(default)]
    combined_findings: String,
    #[serde(default)]
    topics: Vec<TopicWire>,
    #[serde(default)]
    forward_assumption: Option<ResearchForwardAssumption>,
    #[serde(default)]
    leading_indicator: Option<ValidatedLeadingIndicator>,
    #[serde(default)]
    forensic_event: Option<ForensicEventClaim>,
    #[serde(default)]
    pre_profit_observations: Vec<PreProfitObservation>,
    #[serde(default)]
    backfill: Option<BackfillAttempt>,
}

// ---------------------------------------------------------------------------
// The vintage resolver
// ---------------------------------------------------------------------------

/// URL → provenance for the app-side vintage resolution: fresh (this run's
/// evidence ledger) wins over cached (the prior layer); an unknown URL drops
/// the claim. The model never stamps a vintage. It also carries each claim's
/// known ledger ties (`related_condition_id`), keyed by **URL and claim text**,
/// so a claim re-emitted verbatim without its tie can inherit it — and only
/// that claim: a different claim from the same page never borrows a tie. The
/// ties are kept in two pools because a tie inherited onto a *fresh* claim is
/// fresh support the 6g validator honors: a fresh claim may inherit only a tie
/// the model asserted **this run** (a tier-1 or pass output), while a
/// prior-layer tie rides only onto a claim that resolves as cached — freshness
/// resolves by URL, so a prior claim re-emitted verbatim at a page this run
/// fetched for other evidence must not turn its old tie into new support.
struct Provenance {
    fresh: HashMap<String, String>,
    cached: HashMap<String, String>,
    /// (normalized URL, claim key) → the distinct condition ids that exact
    /// claim cited in this run's earlier distillation hops.
    run_ties: HashMap<(String, String), HashSet<String>>,
    /// The same, from the prior layer's claims.
    prior_ties: HashMap<(String, String), HashSet<String>>,
}

/// The claim-text half of a tie key: case- and whitespace-insensitive, so a
/// verbatim re-emission matches through incidental reflow.
fn claim_key(claim: &str) -> String {
    claim
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

impl Provenance {
    /// `known` filters the ties at insertion — a prior claim's tie to a
    /// condition since superseded is no tie, so it can neither inherit nor
    /// make a claim read as ambiguous beside a current one.
    fn build(
        research: &HoldingResearch,
        priors: &[TopicDistillate],
        run_ties: HashMap<(String, String), HashSet<String>>,
        known: &HashSet<&str>,
    ) -> Self {
        let mut prior_ties: HashMap<(String, String), HashSet<String>> = HashMap::new();
        let mut fresh = HashMap::new();
        for topic in &research.topics {
            for pass in &topic.passes {
                for c in &pass.claims {
                    fresh.insert(c.source_url.clone(), c.retrieved_at.clone());
                }
            }
        }
        if let Some(d) = &research.disconfirming {
            for c in &d.claims {
                fresh.insert(c.source_url.clone(), c.retrieved_at.clone());
            }
        }
        let mut cached = HashMap::new();
        for prior in priors {
            for c in &prior.claims {
                let normalized = crate::web_research::store::normalize_url(&c.source_url);
                if let Some(id) = c
                    .related_condition_id
                    .as_deref()
                    .filter(|id| known.contains(id))
                {
                    prior_ties
                        .entry((normalized.clone(), claim_key(&c.claim)))
                        .or_default()
                        .insert(id.to_string());
                }
                cached.insert(normalized, c.vintage.clone());
            }
        }
        Self {
            fresh,
            cached,
            run_ties,
            prior_ties,
        }
    }

    /// Resolve one emitted claim's vintage + cached flag, or `None` (drop).
    fn resolve(&self, source_url: &str) -> Option<(String, bool)> {
        let normalized = crate::web_research::store::normalize_url(source_url);
        if let Some(v) = self.fresh.get(&normalized).or_else(|| self.fresh.get(source_url)) {
            return Some((v.clone(), false));
        }
        self.cached.get(&normalized).map(|v| (v.clone(), true))
    }

    /// The single known ledger tie this exact claim (URL + text) carried, if
    /// unambiguous — two different ties on one claim resolve to none rather
    /// than a guess. A fresh claim reads this run's pool only; a cached one
    /// reads this run's pool first, then the prior layer's.
    fn tie_for(&self, source_url: &str, claim: &str, cached: bool) -> Option<&str> {
        let key = (
            crate::web_research::store::normalize_url(source_url),
            claim_key(claim),
        );
        let from_run = Self::single_tie(&self.run_ties, &key);
        if cached {
            from_run.or_else(|| Self::single_tie(&self.prior_ties, &key))
        } else {
            from_run
        }
    }

    /// One pool's tie for a claim key, only when exactly one id was cited.
    fn single_tie<'a>(
        pool: &'a HashMap<(String, String), HashSet<String>>,
        key: &(String, String),
    ) -> Option<&'a str> {
        let ids = pool.get(key)?;
        if ids.len() == 1 {
            ids.iter().next().map(String::as_str)
        } else {
            None
        }
    }

    fn known(&self, source_url: &str) -> bool {
        self.resolve(source_url).is_some()
    }
}

// ---------------------------------------------------------------------------
// The primitive
// ---------------------------------------------------------------------------

/// Run one holding's Step-6d distillation: deterministic routing, the model
/// calls, and the app-side reconciliation/validation of everything returned.
pub fn distill(model: &dyn DistillModel, inputs: &DistillInputs<'_>) -> Result<DistilledResearch> {
    let mut gaps: Vec<String> = Vec::new();
    let prior_by_key: HashMap<&str, &TopicDistillate> = inputs
        .priors
        .iter()
        .map(|p| (p.topic_key.as_str(), p))
        .collect();
    // Fresh prior objects whose topics were NOT analyzed this run — dormant
    // conditional topics. They join the reduce so the cross-topic
    // reconciliation still updates any claim they share (a dormant object must
    // never re-seed a value another topic superseded —
    // `docs/portfolio-analysis.md` §Starting parameters), and they re-emit
    // with their OWN vintage preserved (dormancy never re-stamps the object).
    let analyzed_keys: HashSet<&str> = inputs
        .research
        .topics
        .iter()
        .filter(|t| !t.passes.is_empty())
        .map(|t| t.topic_key.as_str())
        .collect();
    let dormant_priors: Vec<&TopicDistillate> = inputs
        .priors
        .iter()
        .filter(|p| !analyzed_keys.contains(p.topic_key.as_str()))
        .collect();

    // The full consolidation input's size — every topic's findings + claims,
    // plus its merged prior and the dormant priors riding the reconciliation
    // (`docs/portfolio-analysis.md` §Starting parameters: growth across
    // topics trips the hierarchical path).
    let total: usize = inputs
        .research
        .topics
        .iter()
        .map(|t| topic_input_chars(t, prior_by_key.get(t.topic_key.as_str()).copied()))
        .sum::<usize>()
        + dormant_priors
            .iter()
            .map(|p| topic_input_chars_prior(p))
            .sum::<usize>()
        + inputs
            .research
            .disconfirming
            .as_ref()
            .map(|d| {
                // The disconfirming pass rides the reduce prompt with its full
                // ledger (findings AND claims + source URLs) — count what is
                // actually inserted, or an over-budget input routes single-pass.
                d.findings.chars().count()
                    + d.claims
                        .iter()
                        .map(|c| c.claim.chars().count() + c.source_url.chars().count())
                        .sum::<usize>()
            })
            .unwrap_or(0);

    let single_pass = total <= inputs.input_budget_chars;
    let schema = combined_schema(inputs.role_risk, inputs.overlay_eligible);

    let (wire, shape, tier1_ties) = if single_pass {
        let prompt = reduce_prompt(inputs, None, &prior_by_key, &dormant_priors);
        let body = model
            .distill_call(&format!("distill {}", inputs.symbol), prompt, &schema)
            .context("single-pass distillation failed")?;
        let wire: CombinedWire =
            serde_json::from_str(&body).context("distillation response failed its schema parse")?;
        (wire, DistillShape::SinglePass, HashMap::new())
    } else {
        // Hierarchical: a tier-1 call per topic-tree (the prior merged there),
        // then the reduce over the tier-1 outputs.
        let mut tier1_outputs: Vec<(String, Tier1Wire)> = Vec::new();
        // The ties every earlier hop's output cited — pass bodies, tree
        // reduces, tier-1 calls — harvested app-side so a claim a later hop
        // re-emits verbatim without its tie inherits it.
        let mut ties: HashMap<(String, String), HashSet<String>> = HashMap::new();
        let known: HashSet<&str> = inputs
            .ledger_conditions
            .iter()
            .map(|c| c.condition_id.as_str())
            .collect();
        let mut tier1_calls = 0usize;
        let mut subdistilled_topics = 0usize;
        let mut dropped_passes = 0usize;
        let mut sub_calls_spent = 0usize;
        let t1_schema = tier1_schema();
        for topic in &inputs.research.topics {
            if topic.passes.is_empty() {
                continue;
            }
            let prior = prior_by_key.get(topic.topic_key.as_str()).copied();
            let own = topic_input_chars(topic, prior);
            let body = if own > inputs.input_budget_chars {
                // The within-topic fallback: sub-distill along the pass seam
                // (each pass carrying its findings AND its ledger claims),
                // then a tree-level reduce with the bounded prior retained.
                subdistilled_topics += 1;
                let mut passes: Vec<&crate::portfolio::research::PassFindings> =
                    topic.passes.iter().collect();
                // The cap fail-softs the lowest-priority whole passes (the
                // latest — the root pass is highest priority).
                let allowed = SUB_DISTILLATION_CAP.saturating_sub(sub_calls_spent);
                if passes.len() > allowed {
                    dropped_passes += passes.len() - allowed;
                    gaps.push(format!(
                        "topic {}: {} pass(es) dropped at the sub-distillation cap",
                        topic.topic_key,
                        passes.len() - allowed
                    ));
                    passes.truncate(allowed);
                }
                let mut pass_summaries: Vec<String> = Vec::new();
                for (i, pass) in passes.iter().enumerate() {
                    sub_calls_spent += 1;
                    let prompt = pass_prompt(
                        inputs.symbol,
                        &topic.topic_key,
                        i,
                        pass,
                        inputs.ledger_conditions,
                    );
                    let body = model
                        .distill_call(
                            &format!("distill {} {} pass {}", inputs.symbol, topic.topic_key, i),
                            prompt,
                            &t1_schema,
                        )
                        .context("pass-level sub-distillation failed")?;
                    // The pass→tree-reduce hop: harvest the pass's own ties
                    // (leniently — the body is forwarded verbatim either way).
                    if let Ok(pass_wire) = serde_json::from_str::<Tier1Wire>(&body) {
                        harvest_ties(&pass_wire, &known, &mut ties);
                    }
                    pass_summaries.push(body);
                }
                if pass_summaries.is_empty() {
                    gaps.push(format!(
                        "topic {}: every pass dropped at the sub-distillation cap",
                        topic.topic_key
                    ));
                    continue;
                }
                tier1_calls += 1;
                let prompt = tree_reduce_prompt(
                    inputs.symbol,
                    &topic.topic_key,
                    &pass_summaries,
                    prior,
                    inputs.ledger_conditions,
                );
                model
                    .distill_call(
                        &format!("distill {} {} reduce", inputs.symbol, topic.topic_key),
                        prompt,
                        &t1_schema,
                    )
                    .context("topic tree reduce failed")?
            } else {
                tier1_calls += 1;
                let prompt = tier1_prompt(inputs.symbol, topic, prior, inputs.ledger_conditions);
                model
                    .distill_call(
                        &format!("distill {} {}", inputs.symbol, topic.topic_key),
                        prompt,
                        &t1_schema,
                    )
                    .context("tier-1 distillation failed")?
            };
            let wire: Tier1Wire = serde_json::from_str(&body)
                .context("tier-1 distillation response failed its schema parse")?;
            harvest_ties(&wire, &known, &mut ties);
            tier1_outputs.push((topic.topic_key.clone(), wire));
        }
        let prompt = reduce_prompt(inputs, Some(&tier1_outputs), &prior_by_key, &dormant_priors);
        let body = model
            .distill_call(&format!("distill {} reduce", inputs.symbol), prompt, &schema)
            .context("reduce distillation failed")?;
        let wire: CombinedWire =
            serde_json::from_str(&body).context("reduce response failed its schema parse")?;
        (
            wire,
            DistillShape::Hierarchical {
                tier1_calls,
                subdistilled_topics,
                dropped_passes,
            },
            ties,
        )
    };

    Ok(validate_combined(wire, inputs, shape, gaps, tier1_ties))
}

/// Record the **known** ledger ties one intermediate output's claims cited,
/// keyed by (normalized URL, claim text) — an unknown id is no tie, so it can
/// neither inherit nor make a claim ambiguous.
fn harvest_ties(
    wire: &Tier1Wire,
    known: &HashSet<&str>,
    ties: &mut HashMap<(String, String), HashSet<String>>,
) {
    for c in &wire.claims {
        if let Some(id) = c
            .related_condition_id
            .as_deref()
            .filter(|id| known.contains(id))
        {
            ties.entry((
                crate::web_research::store::normalize_url(&c.source_url),
                claim_key(&c.claim),
            ))
            .or_default()
            .insert(id.to_string());
        }
    }
}

/// A dormant prior object's contribution to the consolidation input's size.
fn topic_input_chars_prior(prior: &TopicDistillate) -> usize {
    prior.summary.chars().count()
        + prior
            .claims
            .iter()
            .map(|c| c.claim.chars().count() + c.source_url.chars().count())
            .sum::<usize>()
}

/// One topic-tree's complete tier-1 input size: its passes' findings and
/// ledger claims plus its bounded prior.
fn topic_input_chars(
    topic: &crate::portfolio::research::TopicResearch,
    prior: Option<&TopicDistillate>,
) -> usize {
    let passes: usize = topic
        .passes
        .iter()
        .map(|p| {
            p.findings.chars().count()
                + p.claims
                    .iter()
                    .map(|c| c.claim.chars().count() + c.source_url.chars().count())
                    .sum::<usize>()
        })
        .sum();
    let prior: usize = prior
        .map(|p| {
            p.summary.chars().count()
                + p.claims
                    .iter()
                    .map(|c| c.claim.chars().count() + c.source_url.chars().count())
                    .sum::<usize>()
        })
        .unwrap_or(0);
    passes + prior
}

/// App-side validation + reconciliation of the combined wire: topic keys must
/// be analyzed topics, claim vintages resolve by URL (fresh wins, cached
/// expires by its own vintage), `related_condition_id` must be a known ledger
/// condition, and every typed field must cite a known source URL. The typed
/// fields are dropped whole on the `role_risk` branch.
fn validate_combined(
    wire: CombinedWire,
    inputs: &DistillInputs<'_>,
    shape: DistillShape,
    mut gaps: Vec<String>,
    tier1_ties: HashMap<(String, String), HashSet<String>>,
) -> DistilledResearch {
    let known_conditions: HashSet<&str> = inputs
        .ledger_conditions
        .iter()
        .map(|c| c.condition_id.as_str())
        .collect();
    let provenance =
        Provenance::build(inputs.research, inputs.priors, tier1_ties, &known_conditions);
    let analyzed: HashSet<&str> = inputs
        .research
        .topics
        .iter()
        .filter(|t| !t.passes.is_empty())
        .map(|t| t.topic_key.as_str())
        .collect();
    // A dormant prior topic re-emits reconciled — accepted like an analyzed
    // one, but its object keeps its OWN vintage (dormancy neither
    // re-researches nor re-stamps; the object still expires on its original
    // clock — `docs/portfolio-analysis.md` §Starting parameters).
    let dormant_vintages: HashMap<&str, &str> = inputs
        .priors
        .iter()
        .filter(|p| !analyzed.contains(p.topic_key.as_str()))
        .map(|p| (p.topic_key.as_str(), p.vintage.as_str()))
        .collect();

    let mut topic_layer: Vec<TopicDistillate> = Vec::new();
    for t in wire.topics {
        let dormant_vintage = dormant_vintages.get(t.topic_key.as_str()).copied();
        if !analyzed.contains(t.topic_key.as_str()) && dormant_vintage.is_none() {
            gaps.push(format!(
                "distillation emitted unknown topic {:?} — dropped",
                t.topic_key
            ));
            continue;
        }
        // One reconciled object per topic: a repeated key keeps the FIRST
        // emitted object and drops the rest with a gap — otherwise both would
        // ride the combined layer while INSERT OR REPLACE persisted only the
        // last, silently diverging the two artifacts.
        if topic_layer.iter().any(|kept| kept.topic_key == t.topic_key) {
            gaps.push(format!(
                "duplicate reconciled object for topic {:?} — dropped (first kept)",
                t.topic_key
            ));
            continue;
        }
        let mut claims = Vec::new();
        let mut dropped = 0usize;
        for c in t.claims {
            let Some((vintage, cached)) = provenance.resolve(&c.source_url) else {
                dropped += 1;
                continue;
            };
            // A cached claim past the window by its OWN vintage never rides
            // forward on the rewritten object's fresh stamp.
            if cached && !vintage_within_window(&vintage, inputs.now) {
                dropped += 1;
                continue;
            }
            // The ledger tie: a known id the model cited stands; an unknown one
            // nulls (never substituted); an omitted one inherits the tie this
            // exact claim (URL + text) carried at an earlier hop of this run —
            // or, for a claim resolving as cached, in the prior layer — so a
            // verbatim re-emission cannot silently decay the link, while a
            // different claim from the same page never borrows one and a prior
            // tie never becomes fresh support (`docs/portfolio-workflow.md`
            // §Step 6d).
            let related = match c.related_condition_id {
                Some(id) if known_conditions.contains(id.as_str()) => Some(id),
                Some(_) => None,
                None => provenance
                    .tie_for(&c.source_url, &c.claim, cached)
                    .filter(|id| known_conditions.contains(id))
                    .map(str::to_string),
            };
            claims.push(DistilledClaim {
                claim: c.claim,
                source_url: crate::web_research::store::normalize_url(&c.source_url),
                vintage,
                cached,
                related_condition_id: related,
            });
        }
        if dropped > 0 {
            gaps.push(format!(
                "topic {}: {dropped} distilled claim(s) dropped (unknown source URL or expired cache vintage)",
                t.topic_key
            ));
        }
        topic_layer.push(TopicDistillate {
            topic_key: t.topic_key,
            vintage: dormant_vintage
                .map(str::to_string)
                .unwrap_or_else(|| inputs.now.to_rfc3339()),
            summary: t.summary,
            claims,
        });
    }

    // The reconciliation contract requires one emitted object per analyzed
    // topic AND per dormant prior (`docs/portfolio-workflow.md` §Step 6d
    // Returns). A topic the model omitted is recorded as a gap and named for
    // the store: its stale row cannot be trusted as reconciled, so the job
    // deletes it rather than letting it seed the next run unreconciled.
    let emitted: HashSet<&str> = topic_layer.iter().map(|t| t.topic_key.as_str()).collect();
    let mut unreconciled_topics: Vec<String> = analyzed
        .iter()
        .chain(dormant_vintages.keys())
        .filter(|k| !emitted.contains(**k))
        .map(|k| (*k).to_string())
        .collect();
    unreconciled_topics.sort();
    unreconciled_topics.dedup();
    for key in &unreconciled_topics {
        gaps.push(format!(
            "topic {key}: distillation emitted no reconciled object — stored seed dropped \
             (the next run seeds this topic cold)"
        ));
    }

    // The typed side-channels: none on role_risk; each must cite a known URL,
    // and each carries the semantic legs its engine consumer demands.
    let mut forward_assumption = None;
    let mut leading_indicator = None;
    let mut forensic_event = None;
    let mut pre_profit_observations = Vec::new();
    let mut backfill = None;
    if !inputs.role_risk {
        forward_assumption = match wire.forward_assumption {
            None => None,
            Some(f) => match assumption_rejection(&f, &provenance, inputs) {
                None => Some(f),
                Some(reason) => {
                    gaps.push(format!("forward assumption dropped ({reason})"));
                    None
                }
            },
        };
        leading_indicator = match wire.leading_indicator {
            None => None,
            Some(mut l) => match indicator_rejection(&l, &provenance, inputs) {
                None => {
                    // App-computed referential integrity (never model-set):
                    // the cited driver id must exist on the current ledger's
                    // key drivers, or the indicator stays visible evidence
                    // with no cap suppression.
                    l.driver_verified = !l.confirms_driver_id.trim().is_empty()
                        && inputs.ledger_key_drivers.iter().any(|d| {
                            !d.driver_id.is_empty() && d.driver_id == l.confirms_driver_id.trim()
                        });
                    if !l.driver_verified {
                        gaps.push(format!(
                            "leading indicator driver reference unverified (id {:?} is not a \
                             current ledger driver) — evidence only, no cap suppression",
                            l.confirms_driver_id
                        ));
                    }
                    Some(l)
                }
                Some(reason) => {
                    gaps.push(format!("leading indicator dropped ({reason})"));
                    None
                }
            },
        };
        forensic_event = match wire.forensic_event {
            None => None,
            Some(e) => match forensic_claim_rejection(&e, &provenance, inputs) {
                None => Some(e),
                Some(reason) => {
                    gaps.push(format!("forensic event claim dropped ({reason})"));
                    None
                }
            },
        };
        if inputs.overlay_eligible {
            for row in wire.pre_profit_observations {
                if provenance.known(&row.source_url) {
                    pre_profit_observations.push(row);
                } else {
                    gaps.push(format!(
                        "pre-profit observation dropped (unknown source URL {})",
                        row.source_url
                    ));
                }
            }
            backfill = wire.backfill;
        } else if !wire.pre_profit_observations.is_empty() {
            gaps.push("pre-profit observations dropped (holding is not overlay-eligible)".into());
        }
    }

    DistilledResearch {
        combined: wire.combined_findings,
        topic_layer,
        unreconciled_topics,
        forward_assumption,
        leading_indicator,
        forensic_event,
        pre_profit_observations,
        backfill,
        shape,
        gaps,
    }
}

/// The fetched-page text behind a typed-channel citation — present only when
/// this holding's own loop fetched the URL this run (cache-served included).
/// Prior-run distilled-claim URLs are provenance-known but carry no page here,
/// so a channel demanding page grounding is implicitly this-run-lineage.
fn run_page_text<'a>(inputs: &DistillInputs<'a>, url: &str) -> Option<&'a str> {
    let normalized = crate::web_research::store::normalize_url(url);
    inputs
        .research
        .page_texts
        .get(&normalized)
        .or_else(|| inputs.research.page_texts.get(url.trim()))
        .map(String::as_str)
}

/// The forward-fact language an assumption's cited page must carry (drafted):
/// a page that only reports a past period, with no guidance / contract /
/// filing vocabulary anywhere, cannot ground a claimed forward fact.
const ASSUMPTION_PAGE_TERMS: &[&str] = &[
    "guidance",
    "guide",
    "guided",
    "guides",
    "outlook",
    "expect",
    "forecast",
    "project",
    "target",
    "contract",
    "agreement",
    "awarded",
    "signed",
    "filed",
    "filing",
];

/// The forward assumption's app-side validation before it may reach the
/// Step-6e conflict policy: a known URL, a finite value, in-range confidence,
/// and **page grounding** — the cited page must have been fetched by this
/// holding's own loop, must name the holding (a cross-issuer guidance figure
/// cannot fill this holding's driver), must carry **forward-fact language**
/// (the drafted lexicon above — a backward-only report grounds no forward
/// fact), and must **state the number**: either `numeric_value` itself
/// appears in the page, or the fact carries its stated range endpoints — both
/// appearing in the page and bounding the value — so a range's midpoint stays
/// usable without literal-matching a number the source never printed (the
/// pre-profit low/high pattern: the model reports what the source states, the
/// app validates the derivation).
fn assumption_rejection(
    f: &ResearchForwardAssumption,
    provenance: &Provenance,
    inputs: &DistillInputs<'_>,
) -> Option<String> {
    if !provenance.known(&f.source_url) {
        return Some("unknown source URL".to_string());
    }
    if !f.numeric_value.is_finite() {
        return Some("non-finite value".to_string());
    }
    if !(0.0..=1.0).contains(&f.confidence) {
        return Some("confidence outside [0, 1]".to_string());
    }
    let Some(page) = run_page_text(inputs, &f.source_url) else {
        return Some("the cited page was not fetched by this holding's loop".to_string());
    };
    if !crate::portfolio::text_names_holding(page, inputs.symbol, inputs.company_name) {
        return Some("the cited page never names the holding".to_string());
    }
    let lower = page.to_ascii_lowercase();
    if !ASSUMPTION_PAGE_TERMS.iter().any(|t| lower.contains(t)) {
        return Some(
            "the cited page carries no forward-fact language (guidance / contract / filing — \
             drafted lexicon)"
                .to_string(),
        );
    }
    match (f.stated_low, f.stated_high) {
        (Some(low), Some(high)) => {
            if !low.is_finite() || !high.is_finite() || low > high {
                return Some(format!("malformed stated range [{low}, {high}]"));
            }
            if !(low..=high).contains(&f.numeric_value) {
                return Some(format!(
                    "value {} lies outside its stated range [{low}, {high}]",
                    f.numeric_value
                ));
            }
            if !crate::portfolio::pre_profit::value_in_text(low, page)
                || !crate::portfolio::pre_profit::value_in_text(high, page)
            {
                return Some(
                    "the cited page never states the range's endpoints".to_string(),
                );
            }
        }
        (None, None) => {
            if !crate::portfolio::pre_profit::value_in_text(f.numeric_value, page) {
                return Some(
                    "the cited page never states the value (a range fact must carry its \
                     stated endpoints)"
                        .to_string(),
                );
            }
        }
        _ => {
            return Some("one stated range endpoint without the other".to_string());
        }
    }
    None
}

/// The leading indicator's app-side validation before its presence may
/// suppress the narrative-hype ceiling: a known URL, a finite value, in-range
/// confidence, an ISO as-of date, **third-party independence** as far as it is
/// deterministically checkable — a host the registry classes as the issuer's
/// own IR site is first-party by construction and rejects
/// (`docs/portfolio-workflow.md §Step 6d` — "countable, dated, third-party") —
/// and **value grounding**: the cited page must have been fetched by this
/// holding's own loop and must state the metric's value (number-boundary; a
/// sub-1 value also tries its percent render). Deliberately **not** a
/// names-the-holding check — a legitimate indicator can be industry-level (a
/// commodity turn, sector shipments) and never name the issuer.
/// `confirms_driver` stays model-attributed context: ledger key drivers are
/// prose, so a deterministic identity check on them would be a fuzzy match,
/// not validation.
fn indicator_rejection(
    l: &ValidatedLeadingIndicator,
    provenance: &Provenance,
    inputs: &DistillInputs<'_>,
) -> Option<String> {
    if !provenance.known(&l.source_url) {
        return Some("unknown source URL".to_string());
    }
    if !l.value.is_finite() {
        return Some("non-finite value".to_string());
    }
    if !(0.0..=1.0).contains(&l.confidence) {
        return Some("confidence outside [0, 1]".to_string());
    }
    if chrono::NaiveDate::parse_from_str(l.as_of.trim(), "%Y-%m-%d").is_err() {
        return Some(format!("non-ISO as-of date {:?}", l.as_of));
    }
    let host = reqwest::Url::parse(l.source_url.trim())
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(crate::web_research::registry::normalize_host)
        })
        .unwrap_or_default();
    if matches!(
        crate::web_research::registry::assess(&host),
        crate::web_research::registry::SourcePolicy::Graded(entry)
            if entry.evidence_kinds.contains(&"company-ir")
    ) {
        return Some(format!(
            "source {host:?} is the issuer's own IR site — a leading indicator must be third-party"
        ));
    }
    // The registry's IR heuristic only sees ir./investor(s). subdomains — an
    // issuer root or newsroom domain is caught by its own identity: a
    // distinctive issuer-name token, the ticker itself (≥3 chars), or the
    // name's acronym (trailing corporate suffixes stripped — so
    // "International Business Machines Corporation" yields `ibm`) inside the
    // host reads first-party (conservative — rejection is fail-soft and
    // gap-logged; aliases and nonliteral domains stay a known residual until
    // an issuer-website field rides the profile).
    let host_lower = host.to_ascii_lowercase();
    let mut probes: Vec<String> = crate::portfolio::distinctive_name_tokens(inputs.company_name)
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    let sym = inputs.symbol.trim().to_ascii_lowercase();
    if sym.len() >= 3 {
        probes.push(sym);
    }
    if let Some(name) = inputs.company_name {
        let words: Vec<&str> = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let mut end = words.len();
        while end > 0 && crate::portfolio::is_generic_name_token(words[end - 1]) {
            end -= 1;
        }
        let acronym: String = words[..end]
            .iter()
            .filter_map(|w| w.chars().next())
            .collect::<String>()
            .to_ascii_lowercase();
        if acronym.len() >= 3 {
            probes.push(acronym);
        }
    }
    if probes.iter().any(|p| host_lower.contains(p)) {
        return Some(format!(
            "source {host:?} carries the issuer's own identity — a leading indicator must be \
             third-party"
        ));
    }
    let Some(page) = run_page_text(inputs, &l.source_url) else {
        return Some("the cited page was not fetched by this holding's loop".to_string());
    };
    let stated = crate::portfolio::pre_profit::value_in_text(l.value, page)
        || (l.value.abs() < 1.0
            && crate::portfolio::pre_profit::value_in_text(l.value * 100.0, page));
    if !stated {
        return Some("the cited page never states the metric's value".to_string());
    }
    None
}

/// The event-class language a fraud citation's page must contain (drafted) —
/// a qualifying page that merely exists must not ground a fabricated record.
/// The page must carry at least **two distinct** terms, and an occurrence
/// inside an `anti-` construction ("anti-fraud", "antifraud") never counts —
/// a genuine litigation release or enforcement action carries several plain
/// terms, while an incidental outreach mention does not.
const FRAUD_PAGE_TERMS: &[&str] = &[
    "fraud",
    "misrepresent",
    "enforcement",
    "charged",
    "complaint",
    "injunction",
    "deceptive",
    "scheme",
    "investigation",
    "subpoena",
    "litigation",
    "securities",
];

/// Whether the page carries `term` outside an `anti-` construction.
fn fraud_term_present(lower_page: &str, term: &str) -> bool {
    let bytes = lower_page.as_bytes();
    lower_page.match_indices(term).any(|(pos, _)| {
        let anti_joined = pos >= 4 && &bytes[pos - 4..pos] == b"anti";
        let anti_sep = pos >= 5
            && &bytes[pos - 5..pos - 1] == b"anti"
            && matches!(bytes[pos - 1], b'-' | b' ');
        !anti_joined && !anti_sep
    })
}

/// The hosts a fraud citation may ride (drafted): the enumerable regulator /
/// court surface — even an advisory claim deserves enumerable producers, so this is an
/// explicit allowlist rather than a registry-class heuristic (every
/// unregistered `.gov` assesses `government-primary`, which is far broader
/// than "a regulator / court document"). The issuer's own filings reach
/// research through EDGAR, so `sec.gov` covers that leg of the contract; a
/// legitimate source outside the list drops fail-soft to a gap and stays
/// visible research history.
const FRAUD_SOURCE_HOSTS: &[&str] = &[
    "sec.gov",
    "justice.gov",
    "ftc.gov",
    "cftc.gov",
    "finra.org",
    "uscourts.gov",
    "occ.gov",
    "fdic.gov",
];

/// The forensic claim's app-side validation (the producer contract, single-homed
/// at `docs/trade-opportunities-workflow.md §Step 5c`): research feeds **only
/// the fraud kind** (restatement / auditor-change are filings-classified,
/// engine-detected — never research-fed), the citation must be a **tier-0
/// primary source** (regulator / court / issuer filing — the registry's tier-0
/// set) **fetched by this holding's own loop**, the fetched page must **name
/// the holding** and carry event-class language (so an unrelated tier-0 page
/// cannot ground a fabricated record), confidence must be in range, and the
/// claimed issuer must identify the holding. Returns the rejection reason, or
/// `None` when the claim stands.
fn forensic_claim_rejection(
    e: &ForensicEventClaim,
    provenance: &Provenance,
    inputs: &DistillInputs<'_>,
) -> Option<String> {
    if !provenance.known(&e.source_url) {
        return Some("unknown source URL".to_string());
    }
    if e.kind.trim() != "fraud" {
        return Some(format!(
            "kind {:?} is filings-classified, never research-fed — only `fraud` rides this channel",
            e.kind
        ));
    }
    let host = reqwest::Url::parse(e.source_url.trim())
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(crate::web_research::registry::normalize_host)
        })
        .unwrap_or_default();
    let allowlisted = FRAUD_SOURCE_HOSTS
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")));
    if !allowlisted {
        return Some(format!(
            "source {host:?} is not on the regulator / court source allowlist (drafted) — \
             the fraud kind accepts only enumerable producers"
        ));
    }
    if !(0.0..=1.0).contains(&e.confidence) {
        return Some("confidence outside [0, 1]".to_string());
    }
    if !crate::portfolio::text_names_holding(&e.issuer, inputs.symbol, inputs.company_name) {
        return Some(format!(
            "issuer {:?} does not identify the holding",
            e.issuer
        ));
    }
    let Some(page) = run_page_text(inputs, &e.source_url) else {
        return Some("the cited page was not fetched by this holding's loop".to_string());
    };
    if !crate::portfolio::text_names_holding(page, inputs.symbol, inputs.company_name) {
        return Some("the cited page never names the holding".to_string());
    }
    let lower = page.to_ascii_lowercase();
    let distinct_terms = FRAUD_PAGE_TERMS
        .iter()
        .filter(|t| fraud_term_present(&lower, t))
        .count();
    if distinct_terms < 2 {
        return Some(
            "the cited page carries no fraud-event language (fewer than two distinct \
             drafted-lexicon terms outside anti- constructions)"
                .to_string(),
        );
    }
    None
}

fn vintage_within_window(vintage: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    chrono::DateTime::parse_from_rfc3339(vintage)
        .map(|t| {
            now.signed_duration_since(t.with_timezone(&chrono::Utc))
                .num_days()
                < crate::portfolio::research::RESEARCH_FRESHNESS_DAYS
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

const MERGE_RULE: &str = "MERGE RULE: fresh findings supersede cached prior findings on conflict \
(newest wins, by claim/metric); deduplicate sources; a prior claim survives only if nothing \
fresh contradicts it. Cite each claim with the exact source URL it came from — URLs from this \
run's research or from the prior findings only.";

fn render_pass(i: usize, pass: &crate::portfolio::research::PassFindings) -> String {
    let mut out = format!("PASS {}:\n{}\n", i + 1, pass.findings);
    if !pass.claims.is_empty() {
        out.push_str("LEDGER CLAIMS:\n");
        for c in &pass.claims {
            out.push_str(&format!("- {} [{}] ({})\n", c.claim, c.source_url, c.retrieved_at));
        }
    }
    out
}

/// A claim's rendered ledger tie, so a re-emission can carry it forward.
fn render_tie(related_condition_id: Option<&str>) -> String {
    related_condition_id
        .map(|id| format!(" (condition {id})"))
        .unwrap_or_default()
}

fn render_prior(prior: &TopicDistillate) -> String {
    let mut out = format!(
        "CACHED PRIOR FINDINGS (vintage {} — merge per the rule):\n{}\n",
        prior.vintage, prior.summary
    );
    for c in &prior.claims {
        out.push_str(&format!(
            "- {} [{}] ({}){}\n",
            c.claim,
            c.source_url,
            c.vintage,
            render_tie(c.related_condition_id.as_deref())
        ));
    }
    out
}

/// The ledger conditions with their app-assigned ids, rendered for citation —
/// the `related_condition_id` channel's front half, the `confirms_driver_id`
/// pattern: the model ties a claim to the condition it bears on by id, the app
/// validates the reference (`docs/portfolio-workflow.md` §Step 6d). Empty on a
/// debut (no ledger to tie to).
fn render_ledger_conditions(conditions: &[crate::portfolio::LedgerCondition]) -> String {
    if conditions.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "\nLEDGER CONDITIONS (set a claim's related_condition_id to the [id] of the condition \
         it bears on — evidence the condition has tripped, is holding, or is at risk; else \
         null; keep a prior claim's tie unless fresh evidence changed it):\n",
    );
    for c in conditions {
        let role = match c.role {
            crate::portfolio::ConditionRole::Falsifier => "FALSIFIER",
            crate::portfolio::ConditionRole::Trigger => "TRIGGER",
        };
        out.push_str(&format!("- [{}] {role}: {}\n", c.condition_id, c.statement));
    }
    out
}

fn render_topic(topic: &crate::portfolio::research::TopicResearch) -> String {
    let mut out = format!("TOPIC {} — {}:\n", topic.topic_key, topic.title);
    for (i, pass) in topic.passes.iter().enumerate() {
        out.push_str(&render_pass(i, pass));
    }
    out
}

fn tier1_prompt(
    symbol: &str,
    topic: &crate::portfolio::research::TopicResearch,
    prior: Option<&TopicDistillate>,
    conditions: &[crate::portfolio::LedgerCondition],
) -> String {
    let mut out = format!(
        "Distill this complete research topic for {symbol} into a compact structured object \
         (summary + sourced claims). {MERGE_RULE}\n"
    );
    out.push_str(&render_ledger_conditions(conditions));
    out.push('\n');
    out.push_str(&render_topic(topic));
    if let Some(prior) = prior {
        out.push_str(&render_prior(prior));
    }
    out
}

fn pass_prompt(
    symbol: &str,
    topic_key: &str,
    i: usize,
    pass: &crate::portfolio::research::PassFindings,
    conditions: &[crate::portfolio::LedgerCondition],
) -> String {
    format!(
        "Distill this single research pass ({topic_key}, pass {}) for {symbol} into a compact \
         structured object (summary + sourced claims). Preserve every distinct sourced fact.\n{}\n{}",
        i + 1,
        render_ledger_conditions(conditions),
        render_pass(i, pass)
    )
}

fn tree_reduce_prompt(
    symbol: &str,
    topic_key: &str,
    pass_summaries: &[String],
    prior: Option<&TopicDistillate>,
    conditions: &[crate::portfolio::LedgerCondition],
) -> String {
    let mut out = format!(
        "Reduce these per-pass distillations of topic {topic_key} for {symbol} into ONE compact \
         structured object (summary + sourced claims). {MERGE_RULE}\n"
    );
    out.push_str(&render_ledger_conditions(conditions));
    out.push('\n');
    for (i, s) in pass_summaries.iter().enumerate() {
        out.push_str(&format!("PASS DISTILLATE {}:\n{s}\n", i + 1));
    }
    if let Some(prior) = prior {
        out.push_str(&render_prior(prior));
    }
    out
}

fn reduce_prompt(
    inputs: &DistillInputs<'_>,
    tier1: Option<&[(String, Tier1Wire)]>,
    prior_by_key: &HashMap<&str, &TopicDistillate>,
    dormant_priors: &[&TopicDistillate],
) -> String {
    let mut out = format!(
        "Consolidate the research on {} into the combined structured object the analysis reads. \
         Emit BOTH the combined findings and the per-topic layer, mutually consistent from ONE \
         reconciliation: resolve cross-topic claim/metric conflicts newest-wins, dedup sources \
         across topics, and update any superseded per-topic claim to the global winner. \
         {MERGE_RULE}\n",
        inputs.symbol
    );
    if !inputs.role_risk {
        out.push_str(
            "\nTyped fields (emit only where a sourced finding genuinely supports one, else null):\n\
             - forward_assumption: ONE sourced forward numeric fact the structured feeds lack \
             (guidance, signed contract, commodity/ASP turn), extracted ONLY from a fetched page \
             that names the company and states the number; a stated range goes in stated_low / \
             stated_high exactly as the page prints them (numeric_value inside the range), a \
             point fact leaves both null with numeric_value as printed. conflict_handling is \
             declared supplement (fills a value the feeds don't carry) or supersede (contradicts \
             a feed value) — the app validates the declaration; it never selects the rule.\n\
             - leading_indicator: a countable, dated, THIRD-PARTY leading indicator research \
             validated that the structured feeds did not carry — cite the fetched page that \
             states the metric's value, never the issuer's own site, and set \
             confirms_driver_id to the [id] of the ledger key driver it confirms from the \
             LEDGER KEY DRIVERS list (an unknown id keeps the indicator as evidence only).\n\
             - forensic_event: a FRAUD event research surfaced, cited to a tier-0 primary \
             source (a regulator / court document or the issuer's own filing) naming this \
             issuer — recorded as ADVISORY attention evidence, never a hard trigger \
             (restatement / auditor-change are detected from classified SEC filings, never \
             emitted here).\n",
        );
        if inputs.overlay_eligible {
            out.push_str(
                "- pre_profit_observations: typed, sourced operating observations (production, \
                 deliveries, bookings/backlog/reservations, guidance, unit economics) extracted \
                 ONLY from source text that states the value; the app computes every comparison.\n\
                 - backfill: the required backfill attempt's checked periods, sources, and \
                 coverage state, where the agenda required one.\n",
            );
        }
        let identified: Vec<&crate::portfolio::KeyDriver> = inputs
            .ledger_key_drivers
            .iter()
            .filter(|d| !d.driver_id.is_empty())
            .collect();
        if !identified.is_empty() {
            out.push_str("\nLEDGER KEY DRIVERS (cite confirms_driver_id from these ids):\n");
            for d in identified {
                out.push_str(&format!("- [{}] {}\n", d.driver_id, d.name));
            }
        }
    }
    // Both branches: the fund ledger's conditions tie the same way, and the
    // role-risk 6g honors the same research-supported leg.
    out.push_str(&render_ledger_conditions(inputs.ledger_conditions));
    out.push('\n');
    match tier1 {
        Some(outputs) => {
            for (key, wire) in outputs {
                out.push_str(&format!("TIER-1 DISTILLATE — {key}:\n{}\n", wire.summary));
                for c in &wire.claims {
                    out.push_str(&format!(
                        "- {} [{}]{}\n",
                        c.claim,
                        c.source_url,
                        render_tie(c.related_condition_id.as_deref())
                    ));
                }
            }
        }
        None => {
            for topic in &inputs.research.topics {
                if topic.passes.is_empty() {
                    continue;
                }
                out.push_str(&render_topic(topic));
                if let Some(prior) = prior_by_key.get(topic.topic_key.as_str()) {
                    out.push_str(&render_prior(prior));
                }
            }
        }
    }
    for prior in dormant_priors {
        out.push_str(&format!(
            "DORMANT PRIOR TOPIC {} (not analyzed this run — re-emit it reconciled: update \
             or drop any claim a fresher topic supersedes, add nothing new):\n{}\n",
            prior.topic_key, prior.summary
        ));
        for c in &prior.claims {
            out.push_str(&format!(
                "- {} [{}] ({}){}\n",
                c.claim,
                c.source_url,
                c.vintage,
                render_tie(c.related_condition_id.as_deref())
            ));
        }
    }
    if let Some(d) = &inputs.research.disconfirming {
        out.push_str("DISCONFIRMING PASS (weigh against the thesis):\n");
        out.push_str(&d.findings);
        out.push('\n');
        for c in &d.claims {
            out.push_str(&format!("- {} [{}]\n", c.claim, c.source_url));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::research::{EvidenceClaim, PassFindings, TopicResearch};
    use std::cell::RefCell;
    use std::sync::Mutex;

    fn utc(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn evidence(claim: &str, url: &str, at: &str) -> EvidenceClaim {
        EvidenceClaim {
            claim: claim.to_string(),
            source_url: url.to_string(),
            retrieved_at: at.to_string(),
            surfaced_by: None,
            annotation: None,
        }
    }

    fn pass(findings: &str, claims: Vec<EvidenceClaim>) -> PassFindings {
        PassFindings {
            findings: findings.to_string(),
            claims,
            followup: None,
            material_forward_fact: false,
            seeded_by: vec![],
            topic_answered: true,
        }
    }

    fn topic(key: &str, passes: Vec<PassFindings>) -> TopicResearch {
        TopicResearch {
            topic_key: key.to_string(),
            title: key.to_string(),
            conditional_reason: None,
            seeded_vintage: None,
            passes,
            skipped: None,
        }
    }

    /// Scripted model: records stages, returns canned bodies in order.
    struct ScriptDistill {
        bodies: Mutex<RefCell<Vec<String>>>,
        stages: Mutex<RefCell<Vec<String>>>,
    }

    impl ScriptDistill {
        fn new(bodies: Vec<Value>) -> Self {
            Self {
                bodies: Mutex::new(RefCell::new(
                    bodies.into_iter().map(|b| b.to_string()).collect(),
                )),
                stages: Mutex::new(RefCell::new(Vec::new())),
            }
        }
        fn stages(&self) -> Vec<String> {
            self.stages.lock().unwrap().borrow().clone()
        }
    }

    impl DistillModel for ScriptDistill {
        fn distill_call(&self, stage: &str, _prompt: String, _schema: &Value) -> Result<String> {
            self.stages
                .lock()
                .unwrap()
                .borrow_mut()
                .push(stage.to_string());
            let guard = self.bodies.lock().unwrap();
            let mut bodies = guard.borrow_mut();
            if bodies.is_empty() {
                anyhow::bail!("distill script exhausted");
            }
            Ok(bodies.remove(0))
        }
    }

    fn research_one_topic() -> HoldingResearch {
        HoldingResearch {
            topics: vec![topic(
                "competitive-position",
                vec![pass(
                    "Widget leads its niche.",
                    vec![evidence(
                        "Q3 revenue was $1.2B",
                        "https://reuters.com/widget",
                        "2026-08-22T10:00:00+00:00",
                    )],
                )],
            )],
            page_texts: [(
                "https://reuters.com/widget".to_string(),
                "Widget Industries reported Q3 revenue of $1.2B and guided bookings of 120 \
                 units."
                    .to_string(),
            )]
            .into(),
            ..Default::default()
        }
    }

    fn combined_body(extra: Value) -> Value {
        let mut base = json!({
            "combined_findings": "Widget executes well.",
            "topics": [{
                "topic_key": "competitive-position",
                "summary": "Leads the niche.",
                "claims": [
                    {"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget"},
                    {"claim": "made-up", "source_url": "https://never.example/x"}
                ]
            }],
            "forward_assumption": null,
            "leading_indicator": null,
            "forensic_event": null
        });
        if let Value::Object(extra) = extra {
            for (k, v) in extra {
                base[k] = v;
            }
        }
        base
    }

    /// Qualitative falsifiers with the given ids — the ledger surface the tie
    /// channel renders and validates against.
    fn conditions(ids: &[&str]) -> Vec<crate::portfolio::LedgerCondition> {
        ids.iter()
            .map(|id| crate::portfolio::LedgerCondition {
                condition_id: id.to_string(),
                role: crate::portfolio::ConditionRole::Falsifier,
                trigger_family: None,
                statement: format!("condition {id} holds"),
                quant: None,
                downgraded_reason: None,
                technology_class: false,
                tripped: false,
                supersedes: None,
                eval_state: None,
            })
            .collect()
    }

    fn inputs<'a>(
        research: &'a HoldingResearch,
        priors: &'a [TopicDistillate],
        conditions: &'a [crate::portfolio::LedgerCondition],
    ) -> DistillInputs<'a> {
        DistillInputs {
            symbol: "WID",
            company_name: Some("Widget Industries"),
            research,
            priors,
            ledger_conditions: conditions,
            ledger_key_drivers: &[],
            role_risk: false,
            overlay_eligible: false,
            input_budget_chars: 100_000,
            now: utc("2026-08-23T00:00:00+00:00"),
        }
    }

    #[test]
    fn single_pass_resolves_vintages_and_drops_unknown_urls() {
        let research = research_one_topic();
        let model = ScriptDistill::new(vec![combined_body(json!({}))]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert_eq!(out.shape, DistillShape::SinglePass);
        assert_eq!(model.stages(), vec!["distill WID"]);
        assert_eq!(out.topic_layer.len(), 1);
        let layer = &out.topic_layer[0];
        // The known claim keeps the ledger's retrieval vintage (fresh, not
        // cached); the fabricated URL drops with a gap.
        assert_eq!(layer.claims.len(), 1);
        assert_eq!(layer.claims[0].vintage, "2026-08-22T10:00:00+00:00");
        assert!(!layer.claims[0].cached);
        assert!(out.gaps.iter().any(|g| g.contains("dropped")));
        // The new layer stamps this run's vintage on the topic object.
        assert_eq!(layer.vintage, "2026-08-23T00:00:00+00:00");
    }

    #[test]
    fn cached_claims_keep_their_own_vintage_and_expire_by_it() {
        let research = research_one_topic();
        let priors = vec![TopicDistillate {
            topic_key: "competitive-position".into(),
            vintage: "2026-08-10T00:00:00+00:00".into(),
            summary: "prior".into(),
            claims: vec![
                DistilledClaim {
                    claim: "carried claim".into(),
                    source_url: "https://ft.com/widget-prior".into(),
                    vintage: "2026-08-10T00:00:00+00:00".into(),
                    cached: true,
                    related_condition_id: None,
                },
                DistilledClaim {
                    claim: "expired claim".into(),
                    source_url: "https://ft.com/widget-old".into(),
                    vintage: "2026-07-01T00:00:00+00:00".into(),
                    cached: true,
                    related_condition_id: None,
                },
            ],
        }];
        let body = combined_body(json!({
            "topics": [{
                "topic_key": "competitive-position",
                "summary": "merged",
                "claims": [
                    {"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget"},
                    {"claim": "carried claim", "source_url": "https://ft.com/widget-prior"},
                    {"claim": "expired claim", "source_url": "https://ft.com/widget-old"}
                ]
            }]
        }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &priors, &[])).unwrap();
        let claims = &out.topic_layer[0].claims;
        assert_eq!(claims.len(), 2, "{claims:?}");
        // The carried claim keeps its ORIGINAL vintage and cached provenance —
        // never renewed by the rewrite.
        let carried = claims.iter().find(|c| c.claim == "carried claim").unwrap();
        assert!(carried.cached);
        assert_eq!(carried.vintage, "2026-08-10T00:00:00+00:00");
        // The expired claim never rides forward on the fresh stamp.
        assert!(claims.iter().all(|c| c.claim != "expired claim"));
    }

    #[test]
    fn related_condition_ids_validate_against_the_ledger() {
        let research = research_one_topic();
        let ids = conditions(&["c1"]);
        let body = combined_body(json!({
            "topics": [{
                "topic_key": "competitive-position",
                "summary": "s",
                "claims": [
                    {"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget",
                     "related_condition_id": "c1"},
                ]
            }]
        }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &[], &ids)).unwrap();
        assert_eq!(
            out.topic_layer[0].claims[0].related_condition_id.as_deref(),
            Some("c1")
        );
        // An unknown id is silently cleared (not a gap — the claim survives).
        let body = combined_body(json!({
            "topics": [{
                "topic_key": "competitive-position",
                "summary": "s",
                "claims": [
                    {"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget",
                     "related_condition_id": "bogus"},
                ]
            }]
        }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &[], &ids)).unwrap();
        assert_eq!(out.topic_layer[0].claims[0].related_condition_id, None);
    }

    // ---- The research→ledger tie channel (2026-08-24 review F3) ----------

    #[test]
    fn ledger_conditions_render_for_citation_in_every_claim_emitting_prompt() {
        let research = research_one_topic();
        let conds = conditions(&["c1"]);
        let topic = &research.topics[0];
        let pass = &topic.passes[0];
        let ins = inputs(&research, &[], &conds);
        let prompts = [
            tier1_prompt("WID", topic, None, &conds),
            pass_prompt("WID", &topic.topic_key, 0, pass, &conds),
            tree_reduce_prompt("WID", &topic.topic_key, &["s".to_string()], None, &conds),
            reduce_prompt(&ins, None, &HashMap::new(), &[]),
        ];
        for p in &prompts {
            assert!(
                p.contains("LEDGER CONDITIONS (set a claim's related_condition_id"),
                "{p}"
            );
            assert!(p.contains("- [c1] FALSIFIER: condition c1 holds"), "{p}");
        }
        // A debut (no ledger) renders no block — there is nothing to tie to.
        let bare = inputs(&research, &[], &[]);
        assert!(!reduce_prompt(&bare, None, &HashMap::new(), &[]).contains("LEDGER CONDITIONS"));
        assert!(!tier1_prompt("WID", topic, None, &[]).contains("LEDGER CONDITIONS"));
        // The role-risk branch renders it too — its 6g honors the same leg.
        let mut rr = inputs(&research, &[], &conds);
        rr.role_risk = true;
        assert!(reduce_prompt(&rr, None, &HashMap::new(), &[]).contains("- [c1] FALSIFIER"));
        // The reduce re-renders a tier-1 claim's tie for the model to carry.
        let tier1 = vec![(
            "competitive-position".to_string(),
            Tier1Wire {
                summary: "t1".into(),
                claims: vec![ClaimWire {
                    claim: "Q3 revenue was $1.2B".into(),
                    source_url: "https://reuters.com/widget".into(),
                    related_condition_id: Some("c1".into()),
                }],
            },
        )];
        let reduce = reduce_prompt(&ins, Some(&tier1), &HashMap::new(), &[]);
        assert!(reduce.contains("[https://reuters.com/widget] (condition c1)"), "{reduce}");
    }

    #[test]
    fn a_prior_claims_tie_renders_and_is_inherited_when_the_re_emission_omits_it() {
        let research = research_one_topic();
        let conds = conditions(&["c1", "c2"]);
        let prior = TopicDistillate {
            topic_key: "competitive-position".into(),
            vintage: "2026-08-10T00:00:00+00:00".into(),
            summary: "prior".into(),
            claims: vec![DistilledClaim {
                claim: "carried claim".into(),
                source_url: "https://cached.example/one".into(),
                vintage: "2026-08-10T00:00:00+00:00".into(),
                cached: true,
                related_condition_id: Some("c1".into()),
            }],
        };
        // Rendered for the model to carry forward…
        assert!(render_prior(&prior).contains("(condition c1)"));
        // …and inherited app-side when the re-emission omits it.
        let priors = vec![prior];
        let body = combined_body(json!({
            "topics": [{
                "topic_key": "competitive-position",
                "summary": "s",
                "claims": [
                    {"claim": "carried claim", "source_url": "https://cached.example/one"},
                ]
            }]
        }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &priors, &conds)).unwrap();
        let carried = out.topic_layer[0]
            .claims
            .iter()
            .find(|c| c.claim == "carried claim")
            .unwrap();
        assert_eq!(carried.related_condition_id.as_deref(), Some("c1"));
        assert!(carried.cached);
    }

    #[test]
    fn an_ambiguous_url_tie_is_never_guessed_and_an_unknown_cited_id_never_substituted() {
        let research = research_one_topic();
        let conds = conditions(&["c1", "c2"]);
        let claim = |text: &str, id: &str| DistilledClaim {
            claim: text.into(),
            source_url: "https://cached.example/one".into(),
            vintage: "2026-08-10T00:00:00+00:00".into(),
            cached: true,
            related_condition_id: Some(id.into()),
        };
        let at = |text: &str, url: &str, id: &str| {
            let mut c = claim(text, id);
            c.source_url = url.into();
            c
        };
        let prior_topic = |key: &str, claims: Vec<DistilledClaim>| TopicDistillate {
            topic_key: key.into(),
            vintage: "2026-08-10T00:00:00+00:00".into(),
            summary: "prior".into(),
            claims,
        };
        // Ties key on the claim (URL + text): "shared" is carried by two prior
        // topics under different ties; "stale" ties a superseded condition.
        let priors = vec![
            prior_topic(
                "competitive-position",
                vec![
                    at("shared", "https://cached.example/one", "c1"),
                    at("solo", "https://cached.example/two", "c1"),
                    at("cited", "https://cached.example/two", "c1"),
                    at("stale", "https://cached.example/two", "gone"),
                ],
            ),
            prior_topic(
                "results-revisions",
                vec![at("shared", "https://cached.example/one", "c2")],
            ),
        ];
        let body = combined_body(json!({
            "topics": [{
                "topic_key": "competitive-position",
                "summary": "s",
                "claims": [
                    {"claim": "shared", "source_url": "https://cached.example/one"},
                    {"claim": "solo", "source_url": "https://cached.example/two"},
                    {"claim": "cited", "source_url": "https://cached.example/two",
                     "related_condition_id": "bogus"},
                    {"claim": "stale", "source_url": "https://cached.example/two"},
                    {"claim": "another fact from the same page",
                     "source_url": "https://cached.example/two"},
                ]
            }]
        }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &priors, &conds)).unwrap();
        let claims = &out.topic_layer[0].claims;
        assert_eq!(claims.len(), 5);
        let tie = |text: &str| {
            claims
                .iter()
                .find(|c| c.claim == text)
                .unwrap()
                .related_condition_id
                .as_deref()
        };
        // The same claim under two different ties: nothing inherits.
        assert_eq!(tie("shared"), None);
        // A verbatim re-emission that omits its tie inherits it.
        assert_eq!(tie("solo"), Some("c1"));
        // An unknown cited id nulls rather than substituting the claim's own
        // known tie — the model asserted something the app can't verify.
        assert_eq!(tie("cited"), None);
        // A tie to a condition no longer on the ledger is no tie at all.
        assert_eq!(tie("stale"), None);
        // A different claim from a tied page never borrows the tie — inherited
        // onto a fresh claim it would be support the 6g validator honors.
        assert_eq!(tie("another fact from the same page"), None);
    }

    #[test]
    fn a_prior_tie_never_rides_onto_a_claim_that_resolves_as_fresh() {
        // Freshness resolves by URL: this run's research fetched the Reuters
        // page (for "Q3 revenue was $1.2B"), so a PRIOR claim re-emitted
        // verbatim at that URL resolves as fresh even though nothing fresh
        // re-established it. Its old tie must not ride along — that would turn
        // a stale tie into support the 6g validator honors. The model citing
        // the tie itself this run is its own fresh assertion and stands.
        let research = research_one_topic();
        let conds = conditions(&["c1"]);
        let priors = vec![TopicDistillate {
            topic_key: "competitive-position".into(),
            vintage: "2026-08-10T00:00:00+00:00".into(),
            summary: "prior".into(),
            claims: vec![DistilledClaim {
                claim: "old reuters claim".into(),
                source_url: "https://reuters.com/widget".into(),
                vintage: "2026-08-10T00:00:00+00:00".into(),
                cached: true,
                related_condition_id: Some("c1".into()),
            }],
        }];
        let run = |cited: bool| {
            let mut re_emitted = json!({"claim": "old reuters claim",
                "source_url": "https://reuters.com/widget"});
            if cited {
                re_emitted["related_condition_id"] = json!("c1");
            }
            let body = combined_body(json!({
                "topics": [{
                    "topic_key": "competitive-position",
                    "summary": "s",
                    "claims": [re_emitted]
                }]
            }));
            let model = ScriptDistill::new(vec![body]);
            let out = distill(&model, &inputs(&research, &priors, &conds)).unwrap();
            out.topic_layer[0].claims[0].clone()
        };
        let omitted = run(false);
        assert!(!omitted.cached, "the URL was fetched this run: {omitted:?}");
        assert_eq!(omitted.related_condition_id, None, "{omitted:?}");
        let cited = run(true);
        assert!(!cited.cached);
        assert_eq!(cited.related_condition_id.as_deref(), Some("c1"));
    }

    #[test]
    fn tier1_ties_survive_the_reduce_hop() {
        // Hierarchical: the tier-1 output ties its claim; the reduce re-emits the
        // claim without the tie (rendered in its prompt, but a model may still
        // drop it) — the app carries it across the hop by URL.
        let research = research_one_topic();
        let conds = conditions(&["c1"]);
        let tied = json!({"summary": "t1", "claims": [
            {"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget",
             "related_condition_id": "c1"}]});
        let untied = json!({"summary": "t1", "claims": [
            {"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget"}]});
        let final_body = || {
            combined_body(json!({
                "topics": [{"topic_key": "competitive-position", "summary": "s1",
                    "claims": [{"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget"}]}]
            }))
        };
        // The topic's own input exceeds the tiny budget: a pass-level call, its
        // tree reduce, then the final reduce — three hops. The tie survives
        // whether it was last seen at the pass (both later hops drop it) or at
        // the tree reduce (only the final reduce drops it).
        for script in [
            vec![tied.clone(), untied.clone(), final_body()],
            vec![untied, tied, final_body()],
        ] {
            let model = ScriptDistill::new(script);
            let mut ins = inputs(&research, &[], &conds);
            ins.input_budget_chars = 10;
            let out = distill(&model, &ins).unwrap();
            assert!(matches!(out.shape, DistillShape::Hierarchical { .. }));
            assert_eq!(
                out.topic_layer[0].claims[0].related_condition_id.as_deref(),
                Some("c1")
            );
            assert!(!out.topic_layer[0].claims[0].cached);
        }
    }

    #[test]
    fn typed_fields_require_known_source_urls_and_role_risk_gets_none() {
        let research = research_one_topic();
        let body = combined_body(json!({
            "forward_assumption": {
                "fact_type": "issued guidance", "numeric_value": 1.2, "units": "USD B",
                "as_of": "2026-08-20", "source_url": "https://reuters.com/widget",
                "confidence": 0.9, "affects": "forward revenue",
                "conflict_handling": "supplement"
            },
            "leading_indicator": {
                "metric_name": "widget bookings", "value": 120.0,
                "direction": "inflecting-up", "as_of": "2026-08-20",
                "source_url": "https://unfetched.example/x", "confidence": 0.8,
                "confirms_driver": "demand"
            },
            "forensic_event": null
        }));
        let model = ScriptDistill::new(vec![body.clone()]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        // The sourced assumption survives; the unsourced indicator drops.
        assert!(out.forward_assumption.is_some());
        assert_eq!(
            out.forward_assumption.as_ref().unwrap().conflict_handling,
            ConflictHandling::Supplement
        );
        assert!(out.leading_indicator.is_none());
        assert!(out.gaps.iter().any(|g| g.contains("leading indicator dropped")));

        // The role_risk branch is pure consolidation: every typed field None.
        let mut ins = inputs(&research, &[], &[]);
        ins.role_risk = true;
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &ins).unwrap();
        assert!(out.forward_assumption.is_none());
        assert!(out.leading_indicator.is_none());
    }

    #[test]
    fn typed_field_confidence_must_be_in_range() {
        let research = research_one_topic();
        let body = combined_body(json!({
            "forward_assumption": {
                "fact_type": "issued guidance", "numeric_value": 4.85, "units": "USD B",
                "as_of": "2026-08-20", "source_url": "https://reuters.com/widget",
                "confidence": 1.5, "affects": "forward revenue",
                "conflict_handling": "supplement"
            },
            "leading_indicator": {
                "metric_name": "widget bookings", "value": 120.0,
                "direction": "inflecting-up", "as_of": "2026-08-20",
                "source_url": "https://reuters.com/widget", "confidence": -0.1,
                "confirms_driver": "demand"
            }
        }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.forward_assumption.is_none());
        assert!(out.leading_indicator.is_none());
        assert!(out.gaps.iter().any(|g| g.contains("forward assumption dropped")));
        assert!(out.gaps.iter().any(|g| g.contains("leading indicator dropped")));
    }

    /// A research fixture whose evidence ledger carries a tier-0 (SEC) page —
    /// the forensic-claim provenance the validation demands — with the fetched
    /// page text retained (the grounding legs read it).
    fn research_with_sec_page() -> HoldingResearch {
        HoldingResearch {
            topics: vec![topic(
                "competitive-position",
                vec![pass(
                    "Widget faces an enforcement action.",
                    vec![
                        evidence(
                            "Q3 revenue was $1.2B",
                            "https://reuters.com/widget",
                            "2026-08-22T10:00:00+00:00",
                        ),
                        evidence(
                            "SEC charged Widget Industries with fraud",
                            "https://www.sec.gov/litigation/widget",
                            "2026-08-22T11:00:00+00:00",
                        ),
                    ],
                )],
            )],
            page_texts: [
                (
                    "https://reuters.com/widget".to_string(),
                    "Widget Industries reported Q3 revenue of $1.2B.".to_string(),
                ),
                (
                    "https://www.sec.gov/litigation/widget".to_string(),
                    "SEC v. Widget Industries — complaint alleging fraud and a deceptive \
                     revenue-recognition scheme."
                        .to_string(),
                ),
            ]
            .into(),
            ..Default::default()
        }
    }

    #[test]
    fn forensic_claims_hold_the_producer_contract() {
        // The producer contract (trade-opportunities-workflow.md §Step 5c):
        // research feeds ONLY fraud, cited tier-0, issuer identifying the
        // holding, confidence in range.
        let research = research_with_sec_page();
        let claim = |kind: &str, url: &str, issuer: &str, confidence: f64| {
            combined_body(json!({
                "topics": [{
                    "topic_key": "competitive-position",
                    "summary": "s",
                    "claims": []
                }],
                "forensic_event": {
                    "kind": kind, "issuer": issuer, "event_date": "2026-08-01",
                    "source_url": url, "confidence": confidence
                }
            }))
        };

        // A restatement kind is filings-classified — never research-fed.
        let model = ScriptDistill::new(vec![claim(
            "restatement",
            "https://www.sec.gov/litigation/widget",
            "Widget Industries",
            0.9,
        )]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());
        assert!(out.gaps.iter().any(|g| g.contains("filings-classified")), "{:?}", out.gaps);

        // A news-outlet citation cannot carry the fraud kind — only the
        // drafted regulator / court host allowlist qualifies.
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://reuters.com/widget",
            "Widget Industries",
            0.9,
        )]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());
        assert!(out.gaps.iter().any(|g| g.contains("source allowlist")), "{:?}", out.gaps);

        // An issuer that does not identify the holding is a cross-issuer claim.
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://www.sec.gov/litigation/widget",
            "Gadget Corp",
            0.9,
        )]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());
        assert!(out.gaps.iter().any(|g| g.contains("does not identify")), "{:?}", out.gaps);

        // Out-of-range confidence rejects.
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://www.sec.gov/litigation/widget",
            "Widget Industries",
            1.2,
        )]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());

        // An UNRELATED tier-0 page cannot ground the record: same URL, but the
        // fetched text never names the holding.
        let mut unrelated = research_with_sec_page();
        unrelated.page_texts.insert(
            "https://www.sec.gov/litigation/widget".to_string(),
            "SEC charges Gadget Corp with fraud in a deceptive scheme.".to_string(),
        );
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://www.sec.gov/litigation/widget",
            "Widget Industries",
            0.9,
        )]);
        let out = distill(&model, &inputs(&unrelated, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("never names the holding")),
            "{:?}",
            out.gaps
        );

        // An incidental mention cannot ground the record — the round-4
        // variant: "securities anti-fraud outreach program" carries the
        // `securities` term, but `fraud` inside an anti- construction never
        // counts, so the page stays below the two-term floor.
        let mut outreach = research_with_sec_page();
        outreach.page_texts.insert(
            "https://www.sec.gov/litigation/widget".to_string(),
            "Widget Industries joined a securities anti-fraud outreach program.".to_string(),
        );
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://www.sec.gov/litigation/widget",
            "Widget Industries",
            0.9,
        )]);
        let out = distill(&model, &inputs(&outreach, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("fewer than two distinct")),
            "{:?}",
            out.gaps
        );

        // A host off the drafted regulator / court allowlist (a macro .gov —
        // and any unregistered .gov, which the registry would grade
        // government-primary) cannot carry the fraud kind: the claim gets
        // enumerable producers, not a class heuristic.
        let mut macro_page = research_with_sec_page();
        macro_page.topics[0].passes[0].claims.push(evidence(
            "rates held",
            "https://www.federalreserve.gov/widget-note",
            "2026-08-22T12:00:00+00:00",
        ));
        macro_page.page_texts.insert(
            "https://www.federalreserve.gov/widget-note".to_string(),
            "Widget Industries fraud complaint enforcement.".to_string(),
        );
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://www.federalreserve.gov/widget-note",
            "Widget Industries",
            0.9,
        )]);
        let out = distill(&model, &inputs(&macro_page, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("source allowlist")),
            "{:?}",
            out.gaps
        );

        // A page naming the holding but carrying no fraud-event language
        // cannot ground the record either.
        let mut no_language = research_with_sec_page();
        no_language.page_texts.insert(
            "https://www.sec.gov/litigation/widget".to_string(),
            "Widget Industries filed its quarterly report on schedule.".to_string(),
        );
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://www.sec.gov/litigation/widget",
            "Widget Industries",
            0.9,
        )]);
        let out = distill(&model, &inputs(&no_language, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("no fraud-event language")),
            "{:?}",
            out.gaps
        );

        // A provenance-known URL whose page was NOT fetched this run (a prior
        // distilled claim's URL) cannot ground the record.
        let mut unfetched = research_with_sec_page();
        unfetched.page_texts.remove("https://www.sec.gov/litigation/widget");
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://www.sec.gov/litigation/widget",
            "Widget Industries",
            0.9,
        )]);
        let out = distill(&model, &inputs(&unfetched, &[], &[])).unwrap();
        assert!(out.forensic_event.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("not fetched by this holding's loop")),
            "{:?}",
            out.gaps
        );

        // The conforming claim survives: fraud, tier-0, this issuer, in range,
        // page fetched + naming the holding + carrying event language.
        let model = ScriptDistill::new(vec![claim(
            "fraud",
            "https://www.sec.gov/litigation/widget",
            "Widget Industries",
            0.9,
        )]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.forensic_event.is_some(), "{:?}", out.gaps);
    }

    #[test]
    fn assumption_and_indicator_hold_their_grounding_legs() {
        // The assumption's cited page must name the holding — a cross-issuer
        // guidance figure must never fill this holding's driver.
        let mut research = research_one_topic();
        research.page_texts.insert(
            "https://reuters.com/widget".to_string(),
            "Gadget Corp guided to 1.2 billion in revenue.".to_string(),
        );
        let assumption = |value: f64, low: Value, high: Value| {
            json!({
                "forward_assumption": {
                    "fact_type": "issued guidance", "numeric_value": value,
                    "stated_low": low, "stated_high": high, "units": "USD B",
                    "as_of": "2026-08-20", "source_url": "https://reuters.com/widget",
                    "confidence": 0.9, "affects": "forward revenue",
                    "conflict_handling": "supplement"
                }
            })
        };
        let model =
            ScriptDistill::new(vec![combined_body(assumption(1.2, Value::Null, Value::Null))]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.forward_assumption.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("never names the holding")),
            "{:?}",
            out.gaps
        );
        // With the holding's own page (the base fixture, which states $1.2B) a
        // point fact survives only when the page states its value.
        let model =
            ScriptDistill::new(vec![combined_body(assumption(1.2, Value::Null, Value::Null))]);
        let out = distill(&model, &inputs(&research_one_topic(), &[], &[])).unwrap();
        assert!(out.forward_assumption.is_some(), "{:?}", out.gaps);
        // A fabricated point value the page never states is rejected — the
        // round-3 gap: 4.85 against a page stating only $1.2B.
        let model =
            ScriptDistill::new(vec![combined_body(assumption(4.85, Value::Null, Value::Null))]);
        let out = distill(&model, &inputs(&research_one_topic(), &[], &[])).unwrap();
        assert!(out.forward_assumption.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("never states the value")),
            "{:?}",
            out.gaps
        );

        // A RANGE fact carries its stated endpoints: both must appear in the
        // page and bound the value — the midpoint itself needn't be printed.
        let mut range_research = research_one_topic();
        range_research.page_texts.insert(
            "https://reuters.com/widget".to_string(),
            "Widget Industries guided full-year revenue to between 4.7 and 5.0 billion."
                .to_string(),
        );
        let model =
            ScriptDistill::new(vec![combined_body(assumption(4.85, json!(4.7), json!(5.0)))]);
        let out = distill(&model, &inputs(&range_research, &[], &[])).unwrap();
        assert!(out.forward_assumption.is_some(), "{:?}", out.gaps);
        // A value outside its stated range rejects.
        let model =
            ScriptDistill::new(vec![combined_body(assumption(6.0, json!(4.7), json!(5.0)))]);
        let out = distill(&model, &inputs(&range_research, &[], &[])).unwrap();
        assert!(out.forward_assumption.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("outside its stated range")),
            "{:?}",
            out.gaps
        );
        // One endpoint without the other rejects.
        let model =
            ScriptDistill::new(vec![combined_body(assumption(4.85, json!(4.7), Value::Null))]);
        let out = distill(&model, &inputs(&range_research, &[], &[])).unwrap();
        assert!(out.forward_assumption.is_none());
        // Endpoints the page never states reject.
        let model =
            ScriptDistill::new(vec![combined_body(assumption(4.85, json!(4.6), json!(5.1)))]);
        let out = distill(&model, &inputs(&range_research, &[], &[])).unwrap();
        assert!(out.forward_assumption.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("range's endpoints")),
            "{:?}",
            out.gaps
        );

        // The indicator's value must be STATED by its cited page (the base
        // fixture's page carries "120"); an unstated value drops.
        let indicator = |value: f64| {
            combined_body(json!({
                "leading_indicator": {
                    "metric_name": "widget bookings", "value": value,
                    "direction": "inflecting-up", "as_of": "2026-08-20",
                    "source_url": "https://reuters.com/widget", "confidence": 0.8,
                    "confirms_driver": "demand"
                }
            }))
        };
        let model = ScriptDistill::new(vec![indicator(120.0)]);
        let out = distill(&model, &inputs(&research_one_topic(), &[], &[])).unwrap();
        assert!(out.leading_indicator.is_some(), "{:?}", out.gaps);
        let model = ScriptDistill::new(vec![indicator(999.0)]);
        let out = distill(&model, &inputs(&research_one_topic(), &[], &[])).unwrap();
        assert!(out.leading_indicator.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("never states the metric's value")),
            "{:?}",
            out.gaps
        );

        // A first-party citation — the issuer's own IR site — is not a
        // third-party indicator and rejects.
        let mut ir_research = research_one_topic();
        ir_research.topics[0].passes[0].claims.push(evidence(
            "bookings of 120 units",
            "https://ir.widget.com/q3",
            "2026-08-22T10:00:00+00:00",
        ));
        ir_research.page_texts.insert(
            "https://ir.widget.com/q3".to_string(),
            "Widget Industries: bookings of 120 units.".to_string(),
        );
        let model = ScriptDistill::new(vec![combined_body(json!({
            "leading_indicator": {
                "metric_name": "widget bookings", "value": 120.0,
                "direction": "inflecting-up", "as_of": "2026-08-20",
                "source_url": "https://ir.widget.com/q3", "confidence": 0.8,
                "confirms_driver": "demand"
            }
        }))]);
        let out = distill(&model, &inputs(&ir_research, &[], &[])).unwrap();
        assert!(out.leading_indicator.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("third-party")),
            "{:?}",
            out.gaps
        );

        // The issuer's ROOT domain (no ir. prefix) is first-party by its own
        // name — a distinctive issuer-name token inside the host rejects.
        let mut root_research = research_one_topic();
        root_research.topics[0].passes[0].claims.push(evidence(
            "bookings of 120 units",
            "https://widget.com/newsroom/q3",
            "2026-08-22T10:00:00+00:00",
        ));
        root_research.page_texts.insert(
            "https://widget.com/newsroom/q3".to_string(),
            "Widget Industries: bookings of 120 units.".to_string(),
        );
        let model = ScriptDistill::new(vec![combined_body(json!({
            "leading_indicator": {
                "metric_name": "widget bookings", "value": 120.0,
                "direction": "inflecting-up", "as_of": "2026-08-20",
                "source_url": "https://widget.com/newsroom/q3", "confidence": 0.8,
                "confirms_driver": "demand"
            }
        }))]);
        let out = distill(&model, &inputs(&root_research, &[], &[])).unwrap();
        assert!(out.leading_indicator.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("issuer's own identity")),
            "{:?}",
            out.gaps
        );

        // A nonliteral issuer domain is caught by the name's ACRONYM once the
        // trailing corporate suffix is stripped — the round-4 IBM case.
        let mut acronym_research = research_one_topic();
        acronym_research.topics[0].passes[0].claims.push(evidence(
            "shipments of 120 units",
            "https://ibm.com/newsroom/q3",
            "2026-08-22T10:00:00+00:00",
        ));
        acronym_research.page_texts.insert(
            "https://ibm.com/newsroom/q3".to_string(),
            "Machines shipped: 120 units.".to_string(),
        );
        let mut ins = inputs(&acronym_research, &[], &[]);
        ins.symbol = "XYZ";
        ins.company_name = Some("International Business Machines Corporation");
        let model = ScriptDistill::new(vec![combined_body(json!({
            "topics": [],
            "leading_indicator": {
                "metric_name": "machine shipments", "value": 120.0,
                "direction": "inflecting-up", "as_of": "2026-08-20",
                "source_url": "https://ibm.com/newsroom/q3", "confidence": 0.8,
                "confirms_driver": "demand"
            }
        }))]);
        let out = distill(&model, &ins).unwrap();
        assert!(out.leading_indicator.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("issuer's own identity")),
            "{:?}",
            out.gaps
        );
    }

    #[test]
    fn indicator_driver_reference_verifies_against_ledger_ids() {
        // Ruled 2026-08-24: only a confirms_driver_id resolving to a current
        // ledger driver grants the cap-suppression anchor; an unknown or
        // absent id keeps the indicator as visible evidence, gap-noted.
        let research = research_one_topic();
        let drivers = vec![crate::portfolio::KeyDriver {
            driver_id: "kd-demand".into(),
            name: "unit demand".into(),
            series: None,
        }];
        let indicator = |id: &str| {
            combined_body(json!({
                "leading_indicator": {
                    "metric_name": "widget bookings", "value": 120.0,
                    "direction": "inflecting-up", "as_of": "2026-08-20",
                    "source_url": "https://reuters.com/widget", "confidence": 0.8,
                    "confirms_driver": "unit demand", "confirms_driver_id": id
                }
            }))
        };
        let mut ins = inputs(&research, &[], &[]);
        ins.ledger_key_drivers = &drivers;
        let model = ScriptDistill::new(vec![indicator("kd-demand")]);
        let out = distill(&model, &ins).unwrap();
        let ind = out.leading_indicator.as_ref().unwrap();
        assert!(ind.driver_verified, "{:?}", out.gaps);
        assert!(!out.gaps.iter().any(|g| g.contains("driver reference unverified")));

        let mut ins = inputs(&research, &[], &[]);
        ins.ledger_key_drivers = &drivers;
        let model = ScriptDistill::new(vec![indicator("kd-bogus")]);
        let out = distill(&model, &ins).unwrap();
        let ind = out.leading_indicator.as_ref().unwrap();
        assert!(!ind.driver_verified);
        assert!(
            out.gaps.iter().any(|g| g.contains("driver reference unverified")),
            "{:?}",
            out.gaps
        );
        // A model-emitted driver_verified is overwritten, never trusted: the
        // schema doesn't carry the field, and validation recomputes it.
        let mut ins = inputs(&research, &[], &[]);
        ins.ledger_key_drivers = &[];
        let model = ScriptDistill::new(vec![combined_body(json!({
            "leading_indicator": {
                "metric_name": "widget bookings", "value": 120.0,
                "direction": "inflecting-up", "as_of": "2026-08-20",
                "source_url": "https://reuters.com/widget", "confidence": 0.8,
                "confirms_driver": "unit demand", "confirms_driver_id": "kd-demand",
                "driver_verified": true
            }
        }))]);
        let out = distill(&model, &ins).unwrap();
        assert!(!out.leading_indicator.as_ref().unwrap().driver_verified);
    }

    #[test]
    fn an_assumption_needs_forward_fact_language_on_the_page() {
        // A backward-only report grounds no forward fact — the page states
        // the number and names the holding, but carries no guidance /
        // contract / filing vocabulary.
        let mut research = research_one_topic();
        research.page_texts.insert(
            "https://reuters.com/widget".to_string(),
            "Widget Industries reported Q3 revenue of $1.2B.".to_string(),
        );
        let model = ScriptDistill::new(vec![combined_body(json!({
            "forward_assumption": {
                "fact_type": "issued guidance", "numeric_value": 1.2,
                "stated_low": null, "stated_high": null, "units": "USD B",
                "as_of": "2026-08-20", "source_url": "https://reuters.com/widget",
                "confidence": 0.9, "affects": "forward revenue",
                "conflict_handling": "supplement"
            }
        }))]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.forward_assumption.is_none());
        assert!(
            out.gaps.iter().any(|g| g.contains("no forward-fact language")),
            "{:?}",
            out.gaps
        );
    }

    #[test]
    fn a_duplicate_topic_object_keeps_the_first_and_gap_logs() {
        let research = research_one_topic();
        let body = combined_body(json!({
            "topics": [
                {
                    "topic_key": "competitive-position",
                    "summary": "first object",
                    "claims": []
                },
                {
                    "topic_key": "competitive-position",
                    "summary": "second object",
                    "claims": []
                }
            ]
        }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert_eq!(out.topic_layer.len(), 1);
        assert_eq!(out.topic_layer[0].summary, "first object");
        assert!(out.unreconciled_topics.is_empty());
        assert!(
            out.gaps.iter().any(|g| g.contains("duplicate reconciled object")),
            "{:?}",
            out.gaps
        );
    }

    #[test]
    fn an_omitted_analyzed_topic_is_named_unreconciled_with_a_gap() {
        let research = research_one_topic();
        // The model emits an empty topics array — the analyzed topic's
        // reconciled object is missing.
        let body = combined_body(json!({ "topics": [] }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert_eq!(out.unreconciled_topics, vec!["competitive-position"]);
        assert!(
            out.gaps.iter().any(|g| g.contains("no reconciled object")),
            "{:?}",
            out.gaps
        );
        // Emitting the topic clears the flag (the base body carries it).
        let model = ScriptDistill::new(vec![combined_body(json!({}))]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.unreconciled_topics.is_empty());
    }

    #[test]
    fn pre_profit_rows_ride_only_the_overlay_eligible_branch() {
        let research = research_one_topic();
        let rows = json!({
            "pre_profit_observations": [{
                "metric_kind": "deliveries", "observation_role": "actual",
                "polarity": "higher-is-better", "numeric_value": 12000.0,
                "units": "vehicles", "period": "2026-06-30",
                "issuer_scope": "consolidated",
                "source_url": "https://reuters.com/widget",
                "published_at": "2026-08-20", "confidence": 0.9
            }],
            "backfill": null
        });
        // Overlay-eligible: the sourced row enters.
        let model = ScriptDistill::new(vec![combined_body(rows.clone())]);
        let mut ins = inputs(&research, &[], &[]);
        ins.overlay_eligible = true;
        let out = distill(&model, &ins).unwrap();
        assert_eq!(out.pre_profit_observations.len(), 1);
        // Not eligible: rows drop with a gap.
        let model = ScriptDistill::new(vec![combined_body(rows)]);
        let out = distill(&model, &inputs(&research, &[], &[])).unwrap();
        assert!(out.pre_profit_observations.is_empty());
        assert!(out.gaps.iter().any(|g| g.contains("not overlay-eligible")));
    }

    #[test]
    fn a_dormant_prior_topic_rides_the_reconciliation_with_its_own_vintage() {
        // The dormant conditional topic's object joins the reduce (so a
        // superseded claim never re-seeds) and re-emits with its ORIGINAL
        // vintage — dormancy never re-stamps the object's clock.
        let research = research_one_topic();
        let priors = vec![TopicDistillate {
            topic_key: "technology-event".into(),
            vintage: "2026-08-10T00:00:00+00:00".into(),
            summary: "dormant tech read".into(),
            claims: vec![DistilledClaim {
                claim: "competitor chip slips".into(),
                source_url: "https://ft.com/tech-prior".into(),
                vintage: "2026-08-10T00:00:00+00:00".into(),
                cached: true,
                related_condition_id: None,
            }],
        }];
        let body = combined_body(json!({
            "topics": [
                {"topic_key": "competitive-position", "summary": "s",
                 "claims": [{"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget"}]},
                {"topic_key": "technology-event", "summary": "dormant, reconciled",
                 "claims": [{"claim": "competitor chip slips", "source_url": "https://ft.com/tech-prior"}]}
            ]
        }));
        let model = ScriptDistill::new(vec![body]);
        let out = distill(&model, &inputs(&research, &priors, &[])).unwrap();
        let dormant = out
            .topic_layer
            .iter()
            .find(|t| t.topic_key == "technology-event")
            .expect("dormant topic re-emits");
        assert_eq!(
            dormant.vintage, "2026-08-10T00:00:00+00:00",
            "dormancy keeps the object's own vintage"
        );
        let carried = &dormant.claims[0];
        assert!(carried.cached);
        assert_eq!(carried.vintage, "2026-08-10T00:00:00+00:00");
        // The analyzed topic still stamps this run's vintage.
        let analyzed = out
            .topic_layer
            .iter()
            .find(|t| t.topic_key == "competitive-position")
            .unwrap();
        assert_eq!(analyzed.vintage, "2026-08-23T00:00:00+00:00");
    }

    #[test]
    fn oversized_input_routes_hierarchical_with_tier1_per_topic() {
        let mut research = research_one_topic();
        research.topics.push(topic(
            "results-revisions",
            vec![pass(
                "Revisions are turning up.",
                vec![evidence(
                    "FY guide raised",
                    "https://apnews.com/widget",
                    "2026-08-21T10:00:00+00:00",
                )],
            )],
        ));
        let tier1 = |summary: &str, url: &str| {
            json!({"summary": summary, "claims": [{"claim": summary, "source_url": url}]})
        };
        let final_body = combined_body(json!({
            "topics": [
                {"topic_key": "competitive-position", "summary": "s1",
                 "claims": [{"claim": "Q3 revenue was $1.2B", "source_url": "https://reuters.com/widget"}]},
                {"topic_key": "results-revisions", "summary": "s2",
                 "claims": [{"claim": "FY guide raised", "source_url": "https://apnews.com/widget"}]}
            ]
        }));
        // Each topic's own input also exceeds the tiny budget, so each rides
        // the pass-seam fallback: a pass-level call + its tree reduce (2 calls
        // per topic), then the final reduce.
        let model = ScriptDistill::new(vec![
            tier1("t1-pass", "https://reuters.com/widget"),
            tier1("t1", "https://reuters.com/widget"),
            tier1("t2-pass", "https://apnews.com/widget"),
            tier1("t2", "https://apnews.com/widget"),
            final_body,
        ]);
        let mut ins = inputs(&research, &[], &[]);
        ins.input_budget_chars = 10; // Force the hierarchical route.
        let out = distill(&model, &ins).unwrap();
        match out.shape {
            DistillShape::Hierarchical {
                tier1_calls,
                subdistilled_topics,
                ..
            } => {
                assert_eq!(tier1_calls, 2);
                // Each topic's own input also exceeds 10 chars, so both went
                // through the pass-seam fallback.
                assert_eq!(subdistilled_topics, 2);
            }
            other => panic!("expected hierarchical, got {other:?}"),
        }
        assert_eq!(out.topic_layer.len(), 2);
        let stages = model.stages();
        assert!(stages.last().unwrap().contains("reduce"), "{stages:?}");
    }

    #[test]
    fn the_sub_distillation_cap_drops_lowest_priority_passes_fail_soft() {
        // One topic with three passes, budget forcing pass-seam sub-distillation,
        // and a cap of 4 shared across the holding: passes beyond the cap drop
        // whole, recorded, never an error.
        let mut passes_vec = Vec::new();
        for i in 0..6 {
            passes_vec.push(pass(
                &format!("pass {i} findings"),
                vec![evidence(
                    &format!("claim {i}"),
                    "https://reuters.com/widget",
                    "2026-08-22T10:00:00+00:00",
                )],
            ));
        }
        // 6 passes but MAX_PASSES_PER_TOPIC=3 normally; construct directly to
        // exercise the cap arithmetic.
        let research = HoldingResearch {
            topics: vec![topic("competitive-position", passes_vec)],
            ..Default::default()
        };
        let tier1 = json!({"summary": "s", "claims": []});
        let final_body = combined_body(json!({"topics": []}));
        let model = ScriptDistill::new(vec![
            tier1.clone(),
            tier1.clone(),
            tier1.clone(),
            tier1.clone(),
            tier1, // the tree reduce
            final_body,
        ]);
        let mut ins = inputs(&research, &[], &[]);
        ins.input_budget_chars = 10;
        let out = distill(&model, &ins).unwrap();
        match out.shape {
            DistillShape::Hierarchical { dropped_passes, .. } => {
                assert_eq!(dropped_passes, 2, "6 passes, cap 4 → 2 dropped");
            }
            other => panic!("expected hierarchical, got {other:?}"),
        }
        assert!(out
            .gaps
            .iter()
            .any(|g| g.contains("sub-distillation cap")));
    }
}
