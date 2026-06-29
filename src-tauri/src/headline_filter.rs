//! The headline-filter stage: a pure structured-in / structured-out boundary
//! that reduces gathered headlines to the bounded set of clustered important
//! stories (`docs/report-workflow.md §Step 7`).
//!
//! This is the filtering half of Step 7. The application layer gathers raw
//! headlines (`news` + the `tavily`/`gdelt`/`fmp_news` adapters) and runs the deterministic
//! `news::dedupe_headlines` pre-pass; this stage then performs the model work —
//! semantic deduplication, relevance scoring, and clustering into major topics —
//! with the fixed low-cost model (GPT-5 mini, `docs/agents.md §Headline
//! Filtering`). The output is the ~10 clustered stories.
//!
//! Mirrors the `agent` / `model_agent` spine: the trait method is synchronous and
//! pure, a deterministic `StubHeadlineFilter` stands in offline, and the real
//! `ModelHeadlineFilter` (its blocking GPT-5 mini HTTP call) replaces the stub
//! behind the same trait. Selecting which of these clusters become the ~5 deeply
//! analyzed topics is research routing's job (Step 8); `pipeline::
//! assemble_research_packet` runs this stage between the news gather and that
//! routing.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::model_agent::extract_openai_envelope;
use crate::news::RawHeadline;
use crate::progress::RunContext;

/// The bounded ceiling on clustered stories the filter returns — the "~10
/// important stories" the Step-7 funnel narrows to. Enforced in
/// `envelope_to_clusters` (the model is asked for at most this many, but the cap
/// is applied deterministically rather than trusted).
pub const MAX_CLUSTERS: usize = 10;

/// The funnel also bounds the *number of headlines* retained across all clusters,
/// not just the cluster count — the "~40 relevant headlines" stage of Step 7's
/// funnel (`docs/report-workflow.md §Step 7`). A deterministic backstop so
/// a lax model response can't carry hundreds of headlines into the Step-8 packet;
/// the prompt asks the model to do the real relevance filtering, so this rarely
/// binds for a well-behaved response.
const MAX_RETAINED_HEADLINES: usize = 40;

/// One clustered important story: a major topic the relevant headlines grouped
/// into, with its member headlines carried through from the gather. `relevance`
/// is the model's 0.0–1.0 score for the cluster's market significance, used to
/// rank and cap the set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeadlineCluster {
    pub topic: String,
    pub summary: String,
    pub relevance: f64,
    pub headlines: Vec<RawHeadline>,
}

/// The headline-filter stage. One method: reduce gathered headlines to the
/// bounded set of clustered stories. Sync and pure, like the `MainAgent` trait —
/// the blocking model HTTP call inside the real adapter is offloaded via
/// `spawn_blocking` at the Tauri command seam.
pub trait HeadlineFilter {
    /// `report_date` is the run's current date (`YYYY-MM-DD`, market/Eastern time) when
    /// known — the recency anchor so the filter can judge "current / prior day" against a
    /// real date rather than guessing from the publish dates it sees. `None` on the
    /// offline/stub path, where the prompt simply omits the date line.
    fn filter(
        &self,
        headlines: Vec<RawHeadline>,
        report_date: Option<&str>,
    ) -> anyhow::Result<Vec<HeadlineCluster>>;
}

/// Deterministic offline stand-in for the real model filter. Groups the input
/// into at most two clusters by simple position so the pipeline and its tests run
/// without live keys: the first half becomes one topic, the rest another. Empty
/// input yields no clusters.
#[derive(Debug, Default)]
pub struct StubHeadlineFilter;

impl HeadlineFilter for StubHeadlineFilter {
    fn filter(
        &self,
        headlines: Vec<RawHeadline>,
        _report_date: Option<&str>,
    ) -> anyhow::Result<Vec<HeadlineCluster>> {
        if headlines.is_empty() {
            return Ok(Vec::new());
        }
        // Honor the real stage's retained-headline ceiling so the double can't hand
        // downstream code an unbounded set the live filter would never produce.
        let mut kept = headlines;
        kept.truncate(MAX_RETAINED_HEADLINES);
        let mid = kept.len().div_ceil(2);
        let mut rest = kept;
        let first = rest.drain(..mid).collect::<Vec<_>>();
        let mut clusters = vec![HeadlineCluster {
            topic: "Macro and policy".to_string(),
            summary: "Headlines grouped by the offline stub filter.".to_string(),
            relevance: 0.8,
            headlines: first,
        }];
        if !rest.is_empty() {
            clusters.push(HeadlineCluster {
                topic: "Markets and sectors".to_string(),
                summary: "Headlines grouped by the offline stub filter.".to_string(),
                relevance: 0.6,
                headlines: rest,
            });
        }
        Ok(clusters)
    }
}

/// OpenAI Chat Completions origin + path — the fixed internal stages call OpenAI
/// directly (the user-selectable agent models live behind `model_agent`). Split into
/// origin + path (like the gated adapters) so a test can redirect the origin at a
/// localhost mock via [`ModelHeadlineFilter::with_base_url`] while still exercising the
/// path the adapter builds.
const OPENAI_BASE: &str = "https://api.openai.com";
const OPENAI_CHAT_PATH: &str = "/v1/chat/completions";

/// The fixed internal model for headline filtering (`docs/agents.md §Headline
/// Filtering`) — non-configurable, distinct from the user-selectable agent models.
const HEADLINE_FILTER_MODEL: &str = "gpt-5-mini";

/// The cluster output itself is small (≤10 topics, each a label + short summary + a few
/// indices), but `gpt-5-mini` is a reasoning model that spends reasoning tokens against
/// this same `max_completion_tokens` budget *before* it emits the structured output. Set
/// too tight, a heavy reasoning pass can exhaust the budget so the response comes back
/// `finish_reason: "length"` with empty content and the parse fails — an intermittent,
/// retry-proof failure distinct from a transient HTTP error. 8192 leaves reasoning
/// headroom over the small output (mirroring the agent stages' reasoning-headroom sizing)
/// while staying well inside the HTTP timeout.
const MAX_TOKENS: u32 = 8192;

const SYSTEM_PROMPT: &str = "You filter a large set of market-news headlines down to the most \
important stories for a market report. You are given a numbered list of headlines. First \
drop headlines that are off-topic, low-signal, or duplicates of others; then group the remaining \
important ones into at most 10 major market-relevant topics. Keep at most about 40 of the most \
relevant headlines in total across all clusters, and assign each headline to at most one topic. \
Each headline shows its source and, when known, its publish date, and the report's current date \
is given above the headlines — judge recency against that date. Judge a topic's market \
importance first, but balance importance with freshness: ensure genuinely recent developments — \
especially from the current and prior day relative to the report date — are represented among the \
topics rather than crowded out by older stories that are merely more heavily covered, and when \
two candidate topics matter similarly, prefer the fresher one. \
For each topic return: a short topic label, a one-to-two-sentence summary of what its headlines \
say, a relevance score from 0.0 to 1.0 for the topic's market significance, and the indices of the \
headlines that belong to it (the numbers in brackets). Return at most 10 clusters — fewer if fewer \
topics matter. Use only the indices provided; never invent headlines.";

const USER_INSTRUCTION: &str =
    "Filter and cluster the following headlines into the most important market stories.";

/// The model's structured return: a list of clusters, each referencing its member
/// headlines by index into the input list rather than echoing their text — so the
/// model can't fabricate or mutate a headline, and the response stays compact.
/// `headline_indices` is `i64` (not `usize`) so a stray negative index from the
/// model is dropped in `envelope_to_clusters` rather than failing the whole parse.
#[derive(Debug, Deserialize)]
struct ClusterEnvelope {
    #[serde(default)]
    clusters: Vec<ClusterRaw>,
}

#[derive(Debug, Deserialize)]
struct ClusterRaw {
    #[serde(default)]
    topic: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    relevance: f64,
    #[serde(default)]
    headline_indices: Vec<i64>,
}

/// JSON Schema for the cluster envelope, used as the OpenAI `json_schema` strict
/// format. All fields required and `additionalProperties` false for strict mode;
/// the ≤10 cap is enforced in `envelope_to_clusters`, not the schema, since strict
/// mode does not honor array-length constraints.
fn cluster_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "clusters": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "topic": { "type": "string" },
                        "summary": { "type": "string" },
                        "relevance": { "type": "number" },
                        "headline_indices": { "type": "array", "items": { "type": "integer" } }
                    },
                    "required": ["topic", "summary", "relevance", "headline_indices"]
                }
            }
        },
        "required": ["clusters"]
    })
}

/// Render the headlines as a numbered list the model references by index. Each line
/// carries the publish date when known, so the filter can weigh freshness (see
/// [`SYSTEM_PROMPT`]) and not just importance; a dateless headline omits it gracefully.
fn format_headlines(headlines: &[RawHeadline]) -> String {
    headlines
        .iter()
        .enumerate()
        .map(|(i, h)| match &h.published {
            Some(date) => format!("[{i}] ({}, {}) {}", h.source, date, h.title),
            None => format!("[{i}] ({}) {}", h.source, h.title),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the GPT-5 mini request: a strict json_schema call whose user message is
/// the standing instruction, the run's current date when known (the recency anchor),
/// plus the numbered headlines.
fn build_request(headlines: &[RawHeadline], report_date: Option<&str>) -> Value {
    let date_line = match report_date {
        Some(d) => format!("Report date (today, US/Eastern): {d}\n\n"),
        None => String::new(),
    };
    let user = format!(
        "{USER_INSTRUCTION}\n\n{date_line}Headlines:\n{}",
        format_headlines(headlines)
    );
    json!({
        "model": HEADLINE_FILTER_MODEL,
        "max_completion_tokens": MAX_TOKENS,
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "headline_clusters", "strict": true, "schema": cluster_schema() }
        },
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": user }
        ]
    })
}

/// Resolve the model's envelope into clusters against the original headlines, and
/// enforce the funnel invariants deterministically rather than trusting the model:
///
/// - **Rank by relevance first**, so the strongest clusters win the caps and win a
///   headline when several clusters claim it.
/// - **Map indices back to `RawHeadline`s**, silently dropping out-of-range or
///   negative indices.
/// - **Dedupe membership** within and across clusters — each headline is kept by
///   the first (highest-relevance) cluster that claims it, so the retained set is
///   distinct stories rather than the same headline repeated.
/// - **Cap retained headlines** at `MAX_RETAINED_HEADLINES` and **clusters** at
///   `MAX_CLUSTERS` (the ~40 → ~10 funnel), dropping clusters left empty.
///
/// Pure, so these invariants are unit-testable without a live call.
fn envelope_to_clusters(env: ClusterEnvelope, headlines: &[RawHeadline]) -> Vec<HeadlineCluster> {
    let mut ranked = env.clusters;
    ranked.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out: Vec<HeadlineCluster> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut retained = 0usize;

    for c in ranked {
        if out.len() >= MAX_CLUSTERS {
            break;
        }
        // Drop malformed clusters before they claim any headlines — strict schema
        // guarantees `topic`/`summary` are strings, not non-empty ones, and a
        // story with no label or summary is useless to Step 8. Checking first
        // leaves the headlines available for a well-formed lower-relevance cluster.
        if c.topic.trim().is_empty() || c.summary.trim().is_empty() {
            continue;
        }
        let mut members = Vec::new();
        for &i in &c.headline_indices {
            if retained >= MAX_RETAINED_HEADLINES {
                break;
            }
            // Negative -> drop (i64 so it's a drop, not a parse failure); out of
            // range -> drop; already claimed by this or a higher cluster -> skip.
            let Ok(idx) = usize::try_from(i) else {
                continue;
            };
            let Some(h) = headlines.get(idx) else {
                continue;
            };
            if !seen.insert(idx) {
                continue;
            }
            members.push(h.clone());
            retained += 1;
        }
        if !members.is_empty() {
            out.push(HeadlineCluster {
                topic: c.topic,
                summary: c.summary,
                relevance: c.relevance.clamp(0.0, 1.0),
                headlines: members,
            });
        }
    }
    out
}

/// Live GPT-5 mini adapter behind the `HeadlineFilter` trait.
pub struct ModelHeadlineFilter {
    api_key: String,
    http: reqwest::blocking::Client,
    /// OpenAI origin, defaulted to [`OPENAI_BASE`]; only a test redirects it at a
    /// localhost mock via [`ModelHeadlineFilter::with_base_url`].
    base_url: String,
    /// Run context for the single tracker row the filter call emits. Defaults to a
    /// no-op (tests / offline smokes); the live command attaches the real one via
    /// [`ModelHeadlineFilter::with_context`].
    progress: Arc<RunContext>,
}

impl ModelHeadlineFilter {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("building the headline-filter HTTP client")?;
        Ok(Self {
            api_key,
            http,
            base_url: OPENAI_BASE.to_string(),
            progress: RunContext::noop(),
        })
    }

    /// Attach a live run context so the filter call streams a request row to the tracker.
    /// Without it the adapter keeps its no-op context.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Redirect the adapter at an alternate API origin (a localhost mock) so the wire
    /// path — including the shared retry/backoff — runs offline. Test-only; a trailing
    /// slash is trimmed so the joined path's leading slash doesn't double up.
    #[cfg(test)]
    fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// Resolve the adapter from the environment, for the live smoke and any caller
    /// that bypasses the gate. Uses the OpenAI key — the fixed internal stages are
    /// always OpenAI (`config::openai_key`).
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().openai_key()?)
    }

    /// POST the request through the shared bounded retry/backoff, matching every other
    /// gated call in the report pipeline (FMP / FRED / BLS / CFTC / Tavily). The clustering
    /// call has no server-side side effect — like Tavily's search POST — so retrying a
    /// transient 429 / 5xx / dropped-connection is safe, and it rides out the intermittent
    /// OpenAI failures that would otherwise wipe this run's news clustering (the pipeline
    /// degrades a failed filter to empty clusters). A non-2xx that survives the retries
    /// stays fatal here.
    fn call(&self, body: &Value) -> Result<Value> {
        let url = format!("{}{OPENAI_CHAT_PATH}", self.base_url);
        let (status, text) = crate::http_retry::send_with_retry("headline-filter", || {
            self.http.post(&url).bearer_auth(&self.api_key).json(body)
        })?;
        if !(200..300).contains(&status) {
            bail!("headline-filter model returned {status}: {text}");
        }
        serde_json::from_str(&text).context("parsing headline-filter response JSON")
    }
}

impl HeadlineFilter for ModelHeadlineFilter {
    fn filter(
        &self,
        headlines: Vec<RawHeadline>,
        report_date: Option<&str>,
    ) -> Result<Vec<HeadlineCluster>> {
        // No headlines, no call — an empty gather has nothing to cluster, and no row.
        if headlines.is_empty() {
            return Ok(Vec::new());
        }
        // One tracker row for the GPT-5-mini clustering call.
        self.progress
            .request_started("OpenAI", "filter", "headline-filter", "Headline filtering");
        let result = (|| -> Result<Vec<HeadlineCluster>> {
            let raw = self.call(&build_request(&headlines, report_date))?;
            let value = extract_openai_envelope(&raw)?;
            let env: ClusterEnvelope = serde_json::from_value(value)
                .context("headline-filter response did not match the schema")?;
            Ok(envelope_to_clusters(env, &headlines))
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "OpenAI",
                "filter",
                "headline-filter",
                "Headline filtering",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "OpenAI",
                "filter",
                "headline-filter",
                "Headline filtering",
                "failed",
                Some(e.to_string()),
            ),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headline(title: &str) -> RawHeadline {
        RawHeadline {
            title: title.into(),
            url: format!("https://example.com/{}", title.replace(' ', "-")),
            source: "example.com".into(),
            published: None,
            snippet: None,
        }
    }

    #[test]
    fn stub_groups_input_into_bounded_clusters() {
        let raw = vec![
            headline("Fed holds rates"),
            headline("Oil spikes"),
            headline("Chips rally"),
            headline("Jobs report beats"),
        ];
        let clusters = StubHeadlineFilter.filter(raw, None).unwrap();
        assert_eq!(clusters.len(), 2);
        assert!(clusters.len() <= MAX_CLUSTERS);
        // Every input headline lands in exactly one cluster.
        let total: usize = clusters.iter().map(|c| c.headlines.len()).sum();
        assert_eq!(total, 4);
        assert!(clusters.iter().all(|c| !c.headlines.is_empty()));
    }

    #[test]
    fn stub_on_empty_input_yields_no_clusters() {
        assert!(StubHeadlineFilter.filter(Vec::new(), None).unwrap().is_empty());
    }

    #[test]
    fn format_headlines_carries_publish_date_when_known() {
        // The filter needs the date to balance freshness against importance; a dated
        // headline renders it inline, an undated one degrades to source-only.
        let dated = RawHeadline {
            published: Some("2026-06-23".into()),
            ..headline("Same-day selloff")
        };
        let undated = headline("Older explainer");
        let rendered = format_headlines(&[dated, undated]);
        assert!(
            rendered.contains("[0] (example.com, 2026-06-23) Same-day selloff"),
            "dated headline shows its date: {rendered}"
        );
        assert!(
            rendered.contains("[1] (example.com) Older explainer"),
            "undated headline omits the date gracefully: {rendered}"
        );
    }

    #[test]
    fn build_request_carries_report_date_anchor_and_omits_it_when_absent() {
        // With a report date, the user message states "today" so the model can judge
        // "current / prior day" against a real anchor (the P1 fix), not guess from the
        // spread of publish dates; without one, the line is omitted, never rendered blank.
        let raw = vec![headline("Same-day selloff")];
        let with = build_request(&raw, Some("2026-06-23"));
        let user_with = with["messages"][1]["content"].as_str().unwrap();
        assert!(
            user_with.contains("Report date (today, US/Eastern): 2026-06-23"),
            "carries the recency anchor: {user_with}"
        );

        let without = build_request(&raw, None);
        let user_without = without["messages"][1]["content"].as_str().unwrap();
        assert!(
            !user_without.contains("Report date"),
            "no anchor line when the date is unknown: {user_without}"
        );
    }

    #[test]
    fn stub_respects_the_retained_headline_ceiling() {
        // The double must not hand downstream code more headlines than the live
        // stage's bounded output would.
        let raw: Vec<RawHeadline> = (0..100).map(|i| headline(&format!("h{i}"))).collect();
        let clusters = StubHeadlineFilter.filter(raw, None).unwrap();
        let total: usize = clusters.iter().map(|c| c.headlines.len()).sum();
        assert_eq!(total, MAX_RETAINED_HEADLINES);
        assert!(clusters.len() <= MAX_CLUSTERS);
    }

    #[test]
    fn headline_cluster_round_trips_through_serde() {
        let c = HeadlineCluster {
            topic: "AI / semiconductors".into(),
            summary: "Capex intentions remain the swing factor.".into(),
            relevance: 0.92,
            headlines: vec![headline("Chips rally")],
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: HeadlineCluster = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn build_request_targets_gpt5_mini_with_strict_schema_and_indexed_headlines() {
        let body = build_request(&[headline("Fed holds rates")], None);
        assert_eq!(body["model"], "gpt-5-mini");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            "headline_clusters"
        );
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        // The user message carries the numbered headline the model references by index.
        let user = body["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("[0] (example.com) Fed holds rates"), "{user}");
    }

    #[test]
    fn envelope_to_clusters_maps_indices_drops_empty_and_clamps_relevance() {
        let headlines: Vec<RawHeadline> = (0..3).map(|i| headline(&format!("h{i}"))).collect();
        let env = ClusterEnvelope {
            clusters: vec![
                ClusterRaw {
                    topic: "valid".into(),
                    summary: "s".into(),
                    relevance: 1.5, // out of range -> clamped to 1.0
                    headline_indices: vec![0, 1],
                },
                ClusterRaw {
                    topic: "out of range".into(),
                    summary: "s".into(),
                    relevance: 0.9,
                    headline_indices: vec![99],
                },
                ClusterRaw {
                    topic: "negative".into(),
                    summary: "s".into(),
                    relevance: 0.9,
                    headline_indices: vec![-1],
                },
            ],
        };
        let out = envelope_to_clusters(env, &headlines);
        // The out-of-range and negative-only clusters resolve to no members -> dropped.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].topic, "valid");
        assert_eq!(out[0].headlines.len(), 2);
        assert!(
            (out[0].relevance - 1.0).abs() < 1e-9,
            "relevance clamped to 1.0"
        );
    }

    #[test]
    fn envelope_to_clusters_caps_at_max_clusters_by_relevance() {
        // 12 clusters, each with its own distinct headline, ascending relevance.
        let headlines: Vec<RawHeadline> = (0..12).map(|i| headline(&format!("h{i}"))).collect();
        let clusters: Vec<ClusterRaw> = (0..12)
            .map(|i| ClusterRaw {
                topic: format!("t{i}"),
                summary: "s".into(),
                relevance: f64::from(i) / 12.0,
                headline_indices: vec![i64::from(i)],
            })
            .collect();
        let out = envelope_to_clusters(ClusterEnvelope { clusters }, &headlines);
        assert_eq!(out.len(), MAX_CLUSTERS, "capped at the cluster ceiling");
        // Sorted by relevance desc, so the highest survives and the lowest is cut.
        assert!(out[0].relevance > out[MAX_CLUSTERS - 1].relevance);
    }

    #[test]
    fn envelope_to_clusters_dedupes_membership_within_and_across_clusters() {
        let headlines: Vec<RawHeadline> = (0..3).map(|i| headline(&format!("h{i}"))).collect();
        let env = ClusterEnvelope {
            clusters: vec![
                // Higher relevance: a within-cluster duplicate of 0, plus 1.
                ClusterRaw {
                    topic: "a".into(),
                    summary: "s".into(),
                    relevance: 0.9,
                    headline_indices: vec![0, 0, 1],
                },
                // Lower relevance: re-claims 0 and 1 (already taken) plus a fresh 2.
                ClusterRaw {
                    topic: "b".into(),
                    summary: "s".into(),
                    relevance: 0.5,
                    headline_indices: vec![0, 1, 2],
                },
            ],
        };
        let out = envelope_to_clusters(env, &headlines);
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].headlines.len(),
            2,
            "a keeps 0 and 1; the duplicate 0 collapses"
        );
        assert_eq!(out[1].headlines.len(), 1, "b keeps only the fresh 2");
        // No headline appears in more than one cluster.
        let mut urls: Vec<String> = out
            .iter()
            .flat_map(|c| c.headlines.iter().map(|h| h.url.clone()))
            .collect();
        let before = urls.len();
        urls.sort();
        urls.dedup();
        assert_eq!(
            urls.len(),
            before,
            "retained headlines are distinct across clusters"
        );
    }

    #[test]
    fn envelope_to_clusters_drops_clusters_with_blank_topic_or_summary() {
        let headlines: Vec<RawHeadline> = (0..3).map(|i| headline(&format!("h{i}"))).collect();
        let env = ClusterEnvelope {
            clusters: vec![
                // Highest relevance but a blank topic -> dropped; its headline 0
                // must stay available rather than being consumed.
                ClusterRaw {
                    topic: "   ".into(),
                    summary: "s".into(),
                    relevance: 0.95,
                    headline_indices: vec![0],
                },
                // Blank summary -> dropped too.
                ClusterRaw {
                    topic: "t".into(),
                    summary: String::new(),
                    relevance: 0.9,
                    headline_indices: vec![1],
                },
                // Well-formed: claims 0 (freed by the dropped cluster) and 2.
                ClusterRaw {
                    topic: "good".into(),
                    summary: "a real summary".into(),
                    relevance: 0.5,
                    headline_indices: vec![0, 2],
                },
            ],
        };
        let out = envelope_to_clusters(env, &headlines);
        assert_eq!(out.len(), 1, "only the well-formed cluster survives");
        assert_eq!(out[0].topic, "good");
        assert_eq!(
            out[0].headlines.len(),
            2,
            "headline 0 was not consumed by the blank cluster"
        );
    }

    #[test]
    fn envelope_to_clusters_caps_total_retained_headlines() {
        // 60 distinct headlines across 5 equal-relevance clusters of 12 -> retained
        // is capped at the funnel's ~40 ceiling, not all 60.
        let headlines: Vec<RawHeadline> = (0..60).map(|i| headline(&format!("h{i}"))).collect();
        let clusters: Vec<ClusterRaw> = (0..5)
            .map(|c| ClusterRaw {
                topic: format!("t{c}"),
                summary: "s".into(),
                relevance: 0.9,
                headline_indices: (c * 12..c * 12 + 12).map(i64::from).collect(),
            })
            .collect();
        let out = envelope_to_clusters(ClusterEnvelope { clusters }, &headlines);
        let total: usize = out.iter().map(|c| c.headlines.len()).sum();
        assert_eq!(
            total, MAX_RETAINED_HEADLINES,
            "total retained headlines capped at ~40"
        );
    }

    #[test]
    fn filter_on_empty_input_returns_no_clusters_without_a_call() {
        // A dummy key is fine: empty input short-circuits before any network call.
        let filter = ModelHeadlineFilter::new("sk-test".into()).unwrap();
        assert!(filter.filter(Vec::new(), None).unwrap().is_empty());
    }

    /// A well-formed OpenAI Chat Completions reply whose `content` is the strict-schema
    /// cluster envelope (itself a JSON string), one cluster claiming headline `[0]`.
    #[cfg(test)]
    const OK_RESPONSE_BODY: &str = r#"{"choices":[{"finish_reason":"stop","message":{"content":"{\"clusters\":[{\"topic\":\"Macro\",\"summary\":\"s\",\"relevance\":0.9,\"headline_indices\":[0]}]}"}}]}"#;

    #[test]
    fn call_retries_a_transient_429_then_parses_the_envelope() {
        use crate::test_http::{Canned, MockHttp};
        // The intermittent failure the user reported: a transient OpenAI 429 on the first
        // attempt, then a clean 200. The shared retry must ride it out and reach the parse
        // rather than failing the call (which the pipeline would degrade to empty clusters,
        // silently dropping this run's news). It must also build the right path.
        let server = MockHttp::serve(vec![
            Canned::Reply {
                status: 429,
                headers: vec![("Retry-After", "0")],
                body: "rate limited",
            },
            Canned::Reply {
                status: 200,
                headers: vec![],
                body: OK_RESPONSE_BODY,
            },
        ]);
        let filter = ModelHeadlineFilter::new("sk-test".into())
            .unwrap()
            .with_base_url(&server.base_url);
        let clusters = filter
            .filter(vec![headline("Fed holds rates")], None)
            .expect("retry rides out the 429 and parses the envelope");
        assert_eq!(server.attempts(), 2, "the 429 was retried exactly once");
        assert_eq!(
            server.request_targets()[0],
            "/v1/chat/completions",
            "the adapter built the Chat Completions path"
        );
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].topic, "Macro");
        assert_eq!(clusters[0].headlines.len(), 1);
    }

    #[test]
    fn call_surfaces_a_non_2xx_that_survives_the_retries() {
        use crate::test_http::{Canned, MockHttp};
        // A persistent 500 (every attempt fails retryably): the call exhausts its retries
        // and surfaces the status, so the pipeline degrades this run to empty clusters with
        // a legible cause rather than hanging.
        let server = MockHttp::serve(vec![
            Canned::Reply { status: 500, headers: vec![], body: "boom 1" },
            Canned::Reply { status: 500, headers: vec![], body: "boom 2" },
            Canned::Reply { status: 500, headers: vec![], body: "boom 3" },
        ]);
        let filter = ModelHeadlineFilter::new("sk-test".into())
            .unwrap()
            .with_base_url(&server.base_url);
        let err = filter
            .filter(vec![headline("Fed holds rates")], None)
            .expect_err("a persistent 500 surfaces as an error");
        assert!(
            err.to_string().contains("500"),
            "the surfaced error names the status: {err}"
        );
    }

    #[test]
    #[ignore = "hits live Tavily + GDELT + OpenAI; set TAVILY_API_KEY + OPENAI_API_KEY"]
    fn headline_filter_funnel_smoke() {
        use crate::news::{dedupe_headlines, CompositeNewsSource, NewsSource};

        let tavily = crate::tavily::TavilyNewsSource::from_env().expect("TAVILY_API_KEY set");
        let gdelt = crate::gdelt::GdeltNewsSource::new().expect("gdelt client");
        let raw = CompositeNewsSource::new(tavily, gdelt)
            .gather(crate::cadence::ReportCadence::from_elapsed(None))
            .expect("gather headlines");
        let deduped = dedupe_headlines(raw);
        assert!(!deduped.is_empty(), "expected headlines to filter");

        let filter = ModelHeadlineFilter::from_env().expect("OPENAI_API_KEY set");
        let clusters = filter.filter(deduped.clone(), None).expect("filter headlines");

        assert!(
            !clusters.is_empty(),
            "the filter produced at least one cluster"
        );
        assert!(
            clusters.len() <= MAX_CLUSTERS,
            "respects the cluster ceiling"
        );
        for c in &clusters {
            assert!(
                !c.headlines.is_empty(),
                "every cluster has member headlines"
            );
            assert!(!c.topic.trim().is_empty(), "every cluster has a topic");
        }
        // The funnel narrows the headline count (not just the cluster count) and
        // keeps the retained set distinct across clusters.
        let mut urls: Vec<String> = clusters
            .iter()
            .flat_map(|c| c.headlines.iter().map(|h| h.url.clone()))
            .collect();
        let total = urls.len();
        assert!(
            total <= MAX_RETAINED_HEADLINES,
            "retained headlines within the ~40 ceiling"
        );
        assert!(
            total < deduped.len(),
            "the funnel narrowed the headline count"
        );
        urls.sort();
        urls.dedup();
        assert_eq!(
            urls.len(),
            total,
            "retained headlines are distinct across clusters"
        );
        eprintln!(
            "headline-filter funnel: {} deduped headlines -> {} clusters, {} retained headlines",
            deduped.len(),
            clusters.len(),
            total
        );
    }
}
