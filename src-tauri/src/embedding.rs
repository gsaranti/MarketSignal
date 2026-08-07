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

/// Byte cap on any text sent to an embedder.
///
/// A byte bound rather than a char or token one because it is the only form that
/// *promises* the provider's limit is respected: every token consumes at least one
/// input byte, so `tokens ≤ bytes`, and 8,000 bytes can never exceed
/// `text-embedding-3-large`'s 8,192-token input limit. A char cap cannot promise it —
/// a multi-byte char can fall back to several byte tokens.
///
/// It is applied inside the adapters, so it holds for **every** call rather than only
/// the ones whose caller remembered: the retrieval query was capped, but the two
/// persistence paths (a report summary, a durable learning) were not, and an
/// oversized one is rejected by the provider and lost rather than truncated.
pub const EMBEDDING_INPUT_MAX_BYTES: usize = 8_000;

/// `text` cut to at most [`EMBEDDING_INPUT_MAX_BYTES`], backed off to a char boundary
/// so the slice can never split a multi-byte character.
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

/// Validate a parsed embedding before it is stored or searched — the shared
/// post-parse check both real adapters run (`docs/local-models.md §The local-model
/// adapter seam`; `docs/report-workflow.md §Step 4`).
///
/// A vector is not trusted because the call returned 200. Two of these are
/// **poisoning** guards rather than hygiene: a non-finite component makes every
/// cosine comparison it touches `NaN`, and a zero vector has no direction, so its
/// cosine against anything is either `NaN` or a meaningless zero — both silently
/// corrupt recall for the life of the row rather than failing. The finiteness check
/// existed only at persistence (`vector_memory::insert_memory`), which guarded the
/// store but let a poisoned *query* vector reach the search unchecked.
///
/// The identity check catches a roster misconfiguration — a daemon serving a
/// different embedder than the one configured produces vectors in a different space,
/// which cosine cannot detect. It is deliberately tolerant: a match is
/// case-insensitive and accepts either name being a prefix of the other, so a tag
/// variant (`qwen3-embedding:4b` vs `…:4b-q4_K_M`) is not a false failure, and a
/// response carrying no model field is not checked at all rather than rejected.
///
/// Dimensionality is deliberately **not** checked. The store is dimension-agnostic
/// by design — `vector_memory` skips rows whose dimension mismatches the query — and
/// that skip is the guard; asserting a configured dimension here would contradict it.
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

/// Whether a served model identity answers for the configured one — see
/// [`validate_embedding`] for why this is prefix-tolerant.
fn model_matches(served: &str, configured: &str) -> bool {
    let (a, b) = (served.trim().to_ascii_lowercase(), configured.trim().to_ascii_lowercase());
    !a.is_empty() && (a.starts_with(&b) || b.starts_with(&a))
}

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
    // The byte cap is applied HERE, in the builder, rather than at each call site:
    // this is what goes on the wire, so capping it is what makes the bound hold for
    // every call — and it keeps the guarantee unit-testable without a live daemon.
    json!({ "model": EMBEDDING_MODEL, "input": bounded_input(text) })
}

/// Pull the vector out of the embeddings response envelope
/// (`data[0].embedding`). Pure, so the envelope contract is unit-testable
/// without a live call. A missing field or a non-numeric component is a typed
/// error rather than a silent partial vector.
fn parse_embedding_response(value: &Value) -> Result<Vec<f32>> {
    // **Exactly one** vector: the request sent one input, so a response carrying
    // several means the envelope is not the one this code reads, and silently taking
    // `data[0]` would pair some other input's vector with this text.
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
        // One tracker row per embedding call. Group "memory" rows follow the
        // currently-running step in the tracker (`App.vue`'s requestStep) — the
        // research step for the Step-4/10 retrieval pulls, the persist step for
        // the Step-17 summary write.
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

/// Ollama's native embeddings endpoint, joined onto the configured daemon base. The
/// local suite reuses this `Embedder` trait so `vector_memory` storage / retrieval is
/// unchanged — only the vector space differs (`docs/local-models.md §The local-model
/// adapter seam`).
const OLLAMA_EMBED_PATH: &str = "/api/embed";

/// Build the Ollama `/api/embed` request body: the roster's embedder model + one
/// input. `keep_alive: -1` holds the embedder resident between calls — the roster's
/// documented stay-resident set is the reasoner *plus* the embedder
/// (`docs/local-models.md §The model roster and per-task routing`).
fn build_local_request(model: &str, text: &str) -> Value {
    // Capped in the builder, as in [`build_request`].
    json!({ "model": model, "input": bounded_input(text), "keep_alive": -1 })
}

/// Pull the vector out of Ollama's `/api/embed` response envelope (`embeddings[0]`,
/// since `input` was a single string). Pure, so the contract is unit-testable without a
/// live daemon. A missing field or a non-numeric component is a typed error rather than
/// a silent partial vector, mirroring [`parse_embedding_response`].
fn parse_local_embedding_response(value: &Value) -> Result<Vec<f32>> {
    // Exactly one vector, for the same reason as [`parse_embedding_response`].
    if let Some(rows) = value.pointer("/embeddings").and_then(Value::as_array) {
        if rows.len() != 1 {
            anyhow::bail!(
                "local embedding response carried {} vectors for one input — malformed \
                 or drifted response",
                rows.len()
            );
        }
    }
    let embedding = value
        .pointer("/embeddings/0")
        .and_then(Value::as_array)
        .context("local embedding response missing embeddings[0]")?;
    embedding
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .context("local embedding response carried a non-numeric component")
        })
        .collect()
}

/// Local embedder behind the `Embedder` trait: the roster's embedding model served by
/// the same Ollama daemon as the chat models (`docs/local-models.md`). Distinct from
/// [`OpenAiEmbedder`] only in endpoint and wire shape; both ride the application
/// layer's `spawn_blocking` seam.
pub struct LocalEmbedder {
    base_url: String,
    model: String,
    http: reqwest::blocking::Client,
    /// Run context for the tracker row each embed call emits; a no-op by default
    /// (tests / offline), the live one attached via [`LocalEmbedder::with_context`].
    progress: Arc<RunContext>,
}

impl LocalEmbedder {
    /// Build the embedder for one daemon endpoint + embedding model id. A trailing
    /// slash on the endpoint is trimmed so the joined path doesn't double up.
    pub fn new(endpoint: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("building the local embedding HTTP client")?;
        Ok(Self {
            // Reuse the local-suite endpoint normalizer so the daemon host and the
            // documented `…/api` base both resolve to one origin (no `/api/api/embed`).
            base_url: crate::local_model::normalize_endpoint(&endpoint.into()),
            model: model.into(),
            http,
            progress: RunContext::noop(),
        })
    }

    /// Attach a live run context so each embed call streams a request row to the tracker.
    pub fn with_context(mut self, ctx: Arc<RunContext>) -> Self {
        self.progress = ctx;
        self
    }

    fn call(&self, body: &Value) -> Result<Value> {
        let resp = self
            .http
            .post(format!("{}{OLLAMA_EMBED_PATH}", self.base_url))
            .json(body)
            .send()
            .context("sending local embedding request")?;
        let status = resp.status();
        let text = resp.text().context("reading local embedding response body")?;
        if !status.is_success() {
            bail!("local embedding model returned {status}: {text}");
        }
        serde_json::from_str(&text).context("parsing local embedding response JSON")
    }
}

impl Embedder for LocalEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.progress
            .request_started("Local", "memory", "embedding", "Memory embedding");
        let result = (|| -> Result<Vec<f32>> {
            let raw = self.call(&build_local_request(&self.model, text))?;
            let vector = parse_local_embedding_response(&raw)?;
            let served = raw.get("model").and_then(Value::as_str);
            validate_embedding(vector, served, &self.model)
        })();
        match &result {
            Ok(_) => self.progress.request_finished(
                "Local",
                "memory",
                "embedding",
                "Memory embedding",
                "ok",
                None,
            ),
            Err(e) => self.progress.request_finished(
                "Local",
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
        assert_eq!(
            parse_embedding_response(&raw).unwrap(),
            vec![0.25, -0.5, 1.0]
        );
    }

    #[test]
    fn parse_embedding_response_errors_on_a_missing_vector() {
        // An empty array is a cardinality failure (0 vectors for 1 input) and says so;
        // an absent envelope has no count to report and names the missing field.
        let err = parse_embedding_response(&json!({ "data": [] })).unwrap_err();
        assert!(err.to_string().contains("0 vectors for one input"), "{err}");
        let err = parse_embedding_response(&json!({})).unwrap_err();
        assert!(err.to_string().contains("data[0].embedding"), "{err}");
    }

    #[test]
    fn a_poisoning_vector_is_rejected_at_the_call_not_only_at_persistence() {
        // These two are the reason the validator exists. A non-finite component makes
        // every cosine comparison it touches NaN; a zero vector has no direction, so
        // its cosine is undefined. Finiteness was checked only at
        // `vector_memory::insert_memory` — guarding the store, while a poisoned QUERY
        // vector reached the search unchecked.
        let err = validate_embedding(vec![0.1, f32::NAN], None, EMBEDDING_MODEL).unwrap_err();
        assert!(err.to_string().contains("non-finite"), "{err}");
        let err = validate_embedding(vec![0.1, f32::INFINITY], None, EMBEDDING_MODEL).unwrap_err();
        assert!(err.to_string().contains("non-finite"), "{err}");
        let err = validate_embedding(vec![0.0, 0.0, 0.0], None, EMBEDDING_MODEL).unwrap_err();
        assert!(err.to_string().contains("zero vector"), "{err}");
        let err = validate_embedding(vec![], None, EMBEDDING_MODEL).unwrap_err();
        assert!(err.to_string().contains("empty vector"), "{err}");
        // A vector with a legitimate zero component is fine — only an ALL-zero one
        // has no direction.
        assert!(validate_embedding(vec![0.0, -0.5, 0.0], None, EMBEDDING_MODEL).is_ok());
    }

    #[test]
    fn a_wrong_embedder_identity_is_rejected_but_tag_variants_are_not() {
        // A daemon serving a different embedder than the one configured produces
        // vectors in a different space, which cosine cannot detect — the vectors look
        // perfectly valid and recall is quietly wrong.
        let err = validate_embedding(vec![0.1], Some("nomic-embed-text"), "qwen3-embedding:4b")
            .unwrap_err();
        assert!(err.to_string().contains("not the configured"), "{err}");

        // Tolerances that must NOT fail: case, surrounding space, and either name
        // being a prefix of the other (a quantization or `:latest` tag).
        for served in [
            "qwen3-embedding:4b",
            "Qwen3-Embedding:4b",
            "  qwen3-embedding:4b  ",
            "qwen3-embedding:4b-q4_K_M",
        ] {
            assert!(
                validate_embedding(vec![0.1], Some(served), "qwen3-embedding:4b").is_ok(),
                "{served} must answer for the configured model"
            );
        }
        // A response with no model field is not checked rather than rejected — the
        // check is only available when the provider echoes an identity.
        assert!(validate_embedding(vec![0.1], None, "qwen3-embedding:4b").is_ok());
        // An empty served identity is not a match claim either.
        assert!(validate_embedding(vec![0.1], Some(""), "qwen3-embedding:4b").is_err());
    }

    #[test]
    fn the_input_byte_cap_holds_for_every_call_and_never_splits_a_char() {
        // The cap is in bytes because that is the only form that promises the
        // provider's token limit is respected (tokens ≤ bytes). The leading ASCII char
        // shifts every 2-byte `é` onto an odd offset, so the cut lands mid-char and
        // must back off to the previous boundary.
        let oversized = format!("a{}", "é".repeat(EMBEDDING_INPUT_MAX_BYTES));
        let capped = bounded_input(&oversized);
        assert_eq!(capped.len(), EMBEDDING_INPUT_MAX_BYTES - 1);
        assert!(capped.is_char_boundary(capped.len()));
        assert_eq!(bounded_input("short"), "short");

        // And both request builders apply it, so every call is bounded — including the
        // two persistence paths (a report summary, a durable learning) that never
        // capped their own input and would have had an oversized one rejected by the
        // provider and lost.
        assert_eq!(
            build_request(&oversized)["input"].as_str().unwrap().len(),
            EMBEDDING_INPUT_MAX_BYTES - 1
        );
        assert_eq!(
            build_local_request("qwen3-embedding:4b", &oversized)["input"]
                .as_str()
                .unwrap()
                .len(),
            EMBEDDING_INPUT_MAX_BYTES - 1
        );
    }

    #[test]
    fn a_response_carrying_several_vectors_for_one_input_is_rejected() {
        // The request sends ONE input, so a multi-vector response means the envelope
        // is not the one this code reads. Silently taking `data[0]` would pair some
        // other input's vector with this text — a wrong vector, stored as if right.
        let err = parse_embedding_response(&json!({
            "data": [ { "embedding": [0.1] }, { "embedding": [0.2] } ]
        }))
        .unwrap_err();
        assert!(err.to_string().contains("2 vectors for one input"), "{err}");
        let err = parse_local_embedding_response(&json!({ "embeddings": [[0.1], [0.2]] }))
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
    fn build_local_request_targets_the_roster_model_with_the_input() {
        let body = build_local_request("qwen3-embedding:4b", "the summary text");
        assert_eq!(body["model"], "qwen3-embedding:4b");
        assert_eq!(body["input"], "the summary text");
        // The stay-resident posture: the embedder never idle-unloads.
        assert_eq!(body["keep_alive"], -1);
    }

    #[test]
    fn parse_local_embedding_response_extracts_the_vector() {
        let raw = json!({ "embeddings": [[0.25, -0.5, 1.0]] });
        assert_eq!(
            parse_local_embedding_response(&raw).unwrap(),
            vec![0.25, -0.5, 1.0]
        );
    }

    #[test]
    fn parse_local_embedding_response_errors_on_a_missing_vector() {
        let err = parse_local_embedding_response(&json!({ "embeddings": [] })).unwrap_err();
        assert!(err.to_string().contains("0 vectors for one input"), "{err}");
        let err = parse_local_embedding_response(&json!({})).unwrap_err();
        assert!(err.to_string().contains("embeddings[0]"), "{err}");
    }

    #[test]
    fn parse_local_embedding_response_errors_on_a_non_numeric_component() {
        let raw = json!({ "embeddings": [[0.25, "oops"]] });
        let err = parse_local_embedding_response(&raw).unwrap_err();
        assert!(err.to_string().contains("non-numeric"), "{err}");
    }

    #[test]
    fn local_embedder_round_trips_a_200_into_a_vector() {
        use crate::test_http::{Canned, MockHttp};
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"embeddings":[[0.1,0.2,0.3]]}"#,
        }]);
        let embedder = LocalEmbedder::new(&server.base_url, "qwen3-embedding:4b").unwrap();
        let v = embedder.embed("risk posture: mixed").unwrap();
        assert_eq!(v, vec![0.1, 0.2, 0.3]);
        assert_eq!(server.request_paths(), vec!["/api/embed".to_string()]);
    }

    #[test]
    fn local_embedder_does_not_double_api_when_endpoint_includes_it() {
        use crate::test_http::{Canned, MockHttp};
        let server = MockHttp::serve(vec![Canned::Reply {
            status: 200,
            headers: vec![],
            body: r#"{"embeddings":[[0.1]]}"#,
        }]);
        // The user entered the documented `…/api` base form.
        let endpoint = format!("{}api", server.base_url);
        let embedder = LocalEmbedder::new(&endpoint, "qwen3-embedding:4b").unwrap();
        embedder.embed("x").unwrap();
        assert_eq!(server.request_paths(), vec!["/api/embed".to_string()]);
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
