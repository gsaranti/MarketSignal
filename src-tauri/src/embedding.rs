//! The embedding seam for vector memory: a pure text → vector boundary.
//!
//! Embeddings are a fixed internal stage — OpenAI `text-embedding-3-large`
//! (`docs/storage.md §Embeddings`, `docs/agents.md §Fixed Internal Models`),
//! non-configurable and distinct from the user-selectable agent models. Mirrors
//! the model-adapter spine (`headline_filter` is the template): the trait method
//! is synchronous and pure, a deterministic `StubEmbedder` stands in offline,
//! and the real `OpenAiEmbedder` (its blocking HTTP call) replaces the stub
//! behind the same trait, inside the same `spawn_blocking` as the rest of the
//! pipeline.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::progress::RunContext;

/// OpenAI embeddings endpoint — fixed internal stages call OpenAI directly.
const OPENAI_EMBEDDINGS_URL: &str = "https://api.openai.com/v1/embeddings";

/// The fixed internal embedding model (`docs/storage.md §Embeddings`).
pub const EMBEDDING_MODEL: &str = "text-embedding-3-large";

/// `text-embedding-3-large`'s native dimension (no `dimensions` reduction is
/// requested). The store itself stays dimension-agnostic — `vector_memory`
/// skips rows whose dimension mismatches a query — so this constant is for the
/// live smoke's assertion, not an enforced schema.
pub const EMBEDDING_DIM: usize = 3072;

/// The embedding stage. One method: text in, vector out. Sync and pure, like
/// the other model-stage traits — the blocking HTTP call inside the real
/// adapter rides the application layer's `spawn_blocking` seam.
pub trait Embedder {
    fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}

/// The stub's small fixed dimension — tests never need 3072 floats.
pub const STUB_EMBEDDING_DIM: usize = 8;

/// Deterministic offline stand-in: folds the text's bytes into a small fixed-
/// dimension vector, so the same text always embeds identically and different
/// texts (almost always) differ — enough for the store's insert/search paths to
/// be exercised without a live key. The constant first component keeps the
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

/// Build the embeddings request body: the fixed model, one input text.
fn build_request(text: &str) -> Value {
    json!({ "model": EMBEDDING_MODEL, "input": text })
}

/// Pull the vector out of the embeddings response envelope
/// (`data[0].embedding`). Pure, so the envelope contract is unit-testable
/// without a live call. A missing field or a non-numeric component is a typed
/// error rather than a silent partial vector.
fn parse_embedding_response(value: &Value) -> Result<Vec<f32>> {
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
    /// Run context for the tracker row each embed call emits. Defaults to a
    /// no-op (tests / offline smokes); the live command attaches the real one
    /// via [`OpenAiEmbedder::with_context`].
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

    /// Attach a live run context so each embed call streams a request row to the
    /// tracker. Without it the adapter keeps its no-op context.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    /// Resolve the adapter from the environment, for the live smoke and any
    /// caller that bypasses the gate. Uses the OpenAI key — embeddings are a
    /// fixed internal OpenAI stage (`config::openai_key`).
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
        // One tracker row per embedding call. Group "memory" buckets the row
        // under the persist step in the tracker (`App.vue`'s requestStep).
        self.progress
            .request_started("OpenAI", "memory", "embedding", "Memory embedding");
        let result = (|| -> Result<Vec<f32>> {
            let raw = self.call(&build_request(text))?;
            parse_embedding_response(&raw)
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
        // Empty text still yields a non-zero vector, so cosine stays defined.
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
        assert_eq!(parse_embedding_response(&raw).unwrap(), vec![0.25, -0.5, 1.0]);
    }

    #[test]
    fn parse_embedding_response_errors_on_a_missing_vector() {
        let err = parse_embedding_response(&json!({ "data": [] })).unwrap_err();
        assert!(err.to_string().contains("data[0].embedding"), "{err}");
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
        assert_eq!(v.len(), EMBEDDING_DIM, "text-embedding-3-large native dimension");
        assert!(v.iter().all(|x| x.is_finite()));
        assert!(v.iter().any(|x| *x != 0.0));
        eprintln!("embedding smoke: {} dims, first 4 = {:?}", v.len(), &v[..4]);
    }
}
