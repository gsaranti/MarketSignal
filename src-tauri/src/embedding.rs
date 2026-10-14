//! The embedding seam for vector memory: a fixed internal stage, OpenAI
//! `text-embedding-3-large` (`docs/storage.md §Embeddings`, `docs/agents.md
//! §Fixed Internal Models`), distinct from the user-selectable agent models.
//! The trait is synchronous; the real adapter's blocking HTTP call rides the
//! pipeline's `spawn_blocking`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::progress::RunContext;

const OPENAI_EMBEDDINGS_URL: &str = "https://api.openai.com/v1/embeddings";

/// The fixed internal embedding model (`docs/storage.md §Embeddings`).
pub const EMBEDDING_MODEL: &str = "text-embedding-3-large";

/// The model's native dimension (no `dimensions` reduction is requested). The
/// store stays dimension-agnostic, so this is for the live smoke's assertion,
/// not an enforced schema.
pub const EMBEDDING_DIM: usize = 3072;

/// Byte cap on any text sent to an embedder. A byte bound, not a char or token
/// one, because it is the only form that promises the provider's limit is
/// respected: every token consumes at least one input byte, so `tokens ≤
/// bytes` and 8,000 bytes can never exceed the 8,192-token input limit.
/// Applied inside the adapters, so it holds for every call rather than only
/// the ones whose caller remembered.
pub const EMBEDDING_INPUT_MAX_BYTES: usize = 8_000;

/// Backed off to a char boundary so the slice can never split a multi-byte
/// character.
pub fn bounded_input(text: &str) -> &str {
    if text.len() <= EMBEDDING_INPUT_MAX_BYTES {
        return text;
    }
    let mut end = EMBEDDING_INPUT_MAX_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The post-parse check before a vector is stored or searched
/// (`docs/report-workflow.md §Step 4`). A non-finite component makes every
/// cosine comparison it touches `NaN` and a zero vector has no direction; both
/// would silently corrupt recall rather than fail. The identity check catches
/// a provider serving a different embedder, whose vectors sit in a space cosine
/// cannot detect; it is deliberately tolerant (case-insensitive, either name a
/// prefix of the other, and an absent model field is not checked). Dimension
/// is deliberately not checked: the store is dimension-agnostic and its skip
/// is the guard.
fn validate_embedding(
    vector: Vec<f32>,
    served_model: Option<&str>,
    configured_model: &str,
) -> Result<Vec<f32>> {
    if let Some(served) = served_model {
        if !model_matches(served, configured_model) {
            anyhow::bail!(
                "embedding response came from {served:?}, not the configured \
                 {configured_model:?} — a different model embeds into a different space, \
                 so the vector is not comparable with the stored ones"
            );
        }
    }
    if vector.is_empty() {
        anyhow::bail!("embedding response carried an empty vector");
    }
    if let Some(bad) = vector.iter().find(|v| !v.is_finite()) {
        anyhow::bail!(
            "embedding response carried a non-finite component ({bad}) — it would make \
             every cosine comparison it touches NaN"
        );
    }
    if vector.iter().all(|v| *v == 0.0) {
        anyhow::bail!(
            "embedding response carried a zero vector — it has no direction, so its \
             cosine against anything is undefined"
        );
    }
    Ok(vector)
}

/// Prefix-tolerant; see [`validate_embedding`].
fn model_matches(served: &str, configured: &str) -> bool {
    let (a, b) = (served.trim().to_ascii_lowercase(), configured.trim().to_ascii_lowercase());
    !a.is_empty() && (a.starts_with(&b) || b.starts_with(&a))
}

/// Sync and pure, like the other model-stage traits; the real adapter's
/// blocking HTTP call rides the application layer's `spawn_blocking` seam.
pub trait Embedder {
    fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}

/// The stub's small fixed dimension — tests never need 3072 floats.
pub const STUB_EMBEDDING_DIM: usize = 8;

/// Deterministic offline stand-in. The constant first component keeps the
/// vector non-zero even for empty text, so cosine similarity stays defined.
#[derive(Debug, Default)]
pub struct StubEmbedder;

impl Embedder for StubEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = vec![0.0f32; STUB_EMBEDDING_DIM];
        v[0] = 1.0;
        for (i, b) in text.bytes().enumerate() {
            v[i % STUB_EMBEDDING_DIM] += f32::from(b) / 255.0;
        }
        Ok(v)
    }
}

fn build_request(text: &str) -> Value {
    // The byte cap is applied here, not at each call site, so the bound holds
    // for every call and stays unit-testable without a live daemon.
    json!({ "model": EMBEDDING_MODEL, "input": bounded_input(text) })
}

/// A missing field or a non-numeric component is a typed error rather than a
/// silent partial vector.
fn parse_embedding_response(value: &Value) -> Result<Vec<f32>> {
    // The request sent one input, so several vectors mean the envelope is not
    // the one this code reads; taking `data[0]` would pair some other input's
    // vector with this text.
    if let Some(rows) = value.pointer("/data").and_then(Value::as_array) {
        if rows.len() != 1 {
            anyhow::bail!(
                "embedding response carried {} vectors for one input — malformed or \
                 drifted response",
                rows.len()
            );
        }
    }
    let embedding = value
        .pointer("/data/0/embedding")
        .and_then(Value::as_array)
        .context("embedding response missing data[0].embedding")?;
    embedding
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .context("embedding response carried a non-numeric component")
        })
        .collect()
}

/// Live `text-embedding-3-large` adapter behind the `Embedder` trait.
pub struct OpenAiEmbedder {
    api_key: String,
    http: reqwest::blocking::Client,
    /// Defaults to a no-op context until [`OpenAiEmbedder::with_context`].
    progress: Arc<RunContext>,
}

impl OpenAiEmbedder {
    pub fn new(api_key: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("building the embedding HTTP client")?;
        Ok(Self {
            api_key,
            http,
            progress: RunContext::noop(),
        })
    }

    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// For the live smoke and any caller that bypasses the gate.
    pub fn from_env() -> Result<Self> {
        Self::new(crate::config::AppConfig::from_env().openai_key()?)
    }

    fn call(&self, body: &Value) -> Result<Value> {
        let resp = self
            .http
            .post(OPENAI_EMBEDDINGS_URL)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .context("sending embedding request")?;
        let status = resp.status();
        let text = resp.text().context("reading embedding response body")?;
        if !status.is_success() {
            bail!("embedding model returned {status}: {text}");
        }
        serde_json::from_str(&text).context("parsing embedding response JSON")
    }
}

impl Embedder for OpenAiEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.progress
            .request_started("OpenAI", "memory", "embedding", "Memory embedding");
        let result = (|| -> Result<Vec<f32>> {
            let raw = self.call(&build_request(text))?;
            let vector = parse_embedding_response(&raw)?;
            let served = raw.get("model").and_then(Value::as_str);
            validate_embedding(vector, served, EMBEDDING_MODEL)
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "OpenAI",
                "memory",
                "embedding",
                "Memory embedding",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "OpenAI",
                "memory",
                "embedding",
                "Memory embedding",
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

    #[test]
    fn stub_is_deterministic_fixed_dimension_and_nonzero() {
        let a = StubEmbedder.embed("oil spiked this week").unwrap();
        let b = StubEmbedder.embed("oil spiked this week").unwrap();
        let c = StubEmbedder.embed("yields fell sharply").unwrap();
        assert_eq!(a.len(), STUB_EMBEDDING_DIM);
        assert_eq!(a, b, "same text embeds identically");
        assert_ne!(a, c, "different texts embed differently");
        let empty = StubEmbedder.embed("").unwrap();
        assert!(empty.iter().any(|v| *v != 0.0));
    }

    #[test]
    fn build_request_targets_the_fixed_model_with_the_input_text() {
        let body = build_request("the summary text");
        assert_eq!(body["model"], EMBEDDING_MODEL);
        assert_eq!(body["input"], "the summary text");
    }

    #[test]
    fn parse_embedding_response_extracts_the_vector() {
        let raw = json!({ "data": [ { "embedding": [0.25, -0.5, 1.0] } ] });
        assert_eq!(
            parse_embedding_response(&raw).unwrap(),
            vec![0.25, -0.5, 1.0]
        );
    }

    #[test]
    fn parse_embedding_response_errors_on_a_missing_vector() {
        let err = parse_embedding_response(&json!({ "data": [] })).unwrap_err();
        assert!(err.to_string().contains("0 vectors for one input"), "{err}");
        let err = parse_embedding_response(&json!({})).unwrap_err();
        assert!(err.to_string().contains("data[0].embedding"), "{err}");
    }

    #[test]
    fn a_poisoning_vector_is_rejected_at_the_call_not_only_at_persistence() {
        let err = validate_embedding(vec![0.1, f32::NAN], None, EMBEDDING_MODEL).unwrap_err();
        assert!(err.to_string().contains("non-finite"), "{err}");
        let err = validate_embedding(vec![0.1, f32::INFINITY], None, EMBEDDING_MODEL).unwrap_err();
        assert!(err.to_string().contains("non-finite"), "{err}");
        let err = validate_embedding(vec![0.0, 0.0, 0.0], None, EMBEDDING_MODEL).unwrap_err();
        assert!(err.to_string().contains("zero vector"), "{err}");
        let err = validate_embedding(vec![], None, EMBEDDING_MODEL).unwrap_err();
        assert!(err.to_string().contains("empty vector"), "{err}");
        // Only an all-zero vector has no direction.
        assert!(validate_embedding(vec![0.0, -0.5, 0.0], None, EMBEDDING_MODEL).is_ok());
    }

    #[test]
    fn a_wrong_embedder_identity_is_rejected_but_tag_variants_are_not() {
        let err = validate_embedding(vec![0.1], Some("text-embedding-3-small"), EMBEDDING_MODEL)
            .unwrap_err();
        assert!(err.to_string().contains("not the configured"), "{err}");

        for served in [
            "text-embedding-3-large",
            "Text-Embedding-3-Large",
            "  text-embedding-3-large  ",
            "text-embedding-3-large-2024",
        ] {
            assert!(
                validate_embedding(vec![0.1], Some(served), EMBEDDING_MODEL).is_ok(),
                "{served} must answer for the configured model"
            );
        }
        assert!(validate_embedding(vec![0.1], None, EMBEDDING_MODEL).is_ok());
        // An empty served identity is not a match claim.
        assert!(validate_embedding(vec![0.1], Some(""), EMBEDDING_MODEL).is_err());
    }

    #[test]
    fn the_input_byte_cap_holds_for_every_call_and_never_splits_a_char() {
        // The leading ASCII char shifts every 2-byte `é` onto an odd offset, so
        // the cut lands mid-char and must back off to the previous boundary.
        let oversized = format!("a{}", "é".repeat(EMBEDDING_INPUT_MAX_BYTES));
        let capped = bounded_input(&oversized);
        assert_eq!(capped.len(), EMBEDDING_INPUT_MAX_BYTES - 1);
        assert!(capped.is_char_boundary(capped.len()));
        assert_eq!(bounded_input("short"), "short");

        assert_eq!(
            build_request(&oversized)["input"].as_str().unwrap().len(),
            EMBEDDING_INPUT_MAX_BYTES - 1
        );
    }

    #[test]
    fn a_response_carrying_several_vectors_for_one_input_is_rejected() {
        let err = parse_embedding_response(&json!({
            "data": [ { "embedding": [0.1] }, { "embedding": [0.2] } ]
        }))
        .unwrap_err();
        assert!(err.to_string().contains("2 vectors for one input"), "{err}");
    }

    #[test]
    fn parse_embedding_response_errors_on_a_non_numeric_component() {
        let raw = json!({ "data": [ { "embedding": [0.25, "oops"] } ] });
        let err = parse_embedding_response(&raw).unwrap_err();
        assert!(err.to_string().contains("non-numeric"), "{err}");
    }

    #[test]
    #[ignore = "hits the live OpenAI embeddings API; set OPENAI_API_KEY"]
    fn embedding_live_smoke() {
        let embedder = OpenAiEmbedder::from_env().expect("OPENAI_API_KEY set");
        let v = embedder
            .embed("Risk posture: mixed. Market cycle: late-cycle. Thesis stance: uncertain.")
            .expect("live embedding call");
        assert_eq!(
            v.len(),
            EMBEDDING_DIM,
            "text-embedding-3-large native dimension"
        );
        assert!(v.iter().all(|x| x.is_finite()));
        assert!(v.iter().any(|x| *x != 0.0));
        eprintln!("embedding smoke: {} dims, first 4 = {:?}", v.len(), &v[..4]);
    }
}
