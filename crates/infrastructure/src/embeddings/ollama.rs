use std::sync::Arc;

use async_trait::async_trait;
use domain::{EmbeddingError, EmbeddingService};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::{sync::Semaphore, task::JoinSet};

/// Default embedding model name used when `OLLAMA_EMBED_MODEL` is unset or blank.
///
/// Appears exactly once on the Rust side; both `resolve_embedding_model` and
/// `OllamaEmbeddingConfig::default` reference this constant so a future default
/// change touches one location.
///
/// De-facto app default is `qwen3-embedding:4b` (2560-dim, model-keyed collection
/// `skills__qwen3-embedding-4b`). The vector dimension is always discovered live
/// via `discover_dimension()`, never hardcoded against this name.
const DEFAULT_EMBEDDING_MODEL: &str = "qwen3-embedding:4b";

/// Resolves the embedding model name from an optional raw env value.
///
/// `None` (env var unset) or `Some("")` / `Some("  ")` (blank, emitted by
/// docker-compose interpolation when the host var is unset, e.g.
/// `OLLAMA_EMBED_MODEL: ${OLLAMA_EMBED_MODEL:-}`) both return the default
/// `"qwen3-embedding:4b"`. Any non-blank value is returned as-is after trimming
/// surrounding whitespace.
///
/// This pure function exists so `#240` can test the resolution logic without
/// touching the global process environment.
pub fn resolve_embedding_model(raw: Option<&str>) -> String {
    match raw {
        Some(v) if !v.trim().is_empty() => v.trim().to_owned(),
        _ => DEFAULT_EMBEDDING_MODEL.to_owned(),
    }
}

/// Reads the configured embedding model name from the process environment.
///
/// Delegates to [`resolve_embedding_model`] with the current value of
/// `OLLAMA_EMBED_MODEL`. Unset or blank returns `"nomic-embed-text"` so existing
/// deployments are unaffected. Set to `"qwen3-embedding:4b"` to activate the
/// qwen local-dense-retrieval arm.
///
/// Blank is treated as absent — docker-compose interpolation emits an empty string
/// when the host env var is unset (e.g. `OLLAMA_EMBED_MODEL: ${OLLAMA_EMBED_MODEL:-}`).
pub fn embedding_model_from_env() -> String {
    resolve_embedding_model(std::env::var("OLLAMA_EMBED_MODEL").ok().as_deref())
}

/// Conservative character budget for a single embedding input.
///
/// The embedding model processes each input in one physical batch (Ollama /
/// llama.cpp default `n_batch` = 2048 tokens for embeddings; unlike generation,
/// the whole input must fit one batch because there is no cross-batch KV reuse
/// when pooling to a single vector). Estimating ~2 characters per token in the
/// worst case (dense code/markdown) keeps a 4000-character cap safely under the
/// 2048-token limit. Inputs longer than this are truncated with a `warn!` rather
/// than allowed to fail the embed call (see `embed_with_timeout`).
const MAX_EMBED_INPUT_CHARS: usize = 4000;

/// Caps an embedding input to the model's safe single-batch window, truncating
/// on a UTF-8 char boundary and emitting a `warn!` when it does. Returns the text
/// unchanged (cloned) when it already fits. Truncation is loud, never silent: a
/// fixed embedding window legitimately bounds the input, but operators are told so
/// they can shorten the source if full-fidelity embedding matters.
fn cap_to_embed_window(text: &str) -> String {
    let char_count = text.chars().count();
    if char_count <= MAX_EMBED_INPUT_CHARS {
        return text.to_owned();
    }
    tracing::warn!(
        original_chars = char_count,
        truncated_to = MAX_EMBED_INPUT_CHARS,
        "embedding input exceeded the safe window; truncating before embed \
         (this caps the vector to the model's batch limit — shorten the source \
         skill/subunit to embed it in full)"
    );
    text.chars().take(MAX_EMBED_INPUT_CHARS).collect()
}

/// Identity and dimension of the active embedding model, discovered from the
/// live Ollama service via a real embed call.
///
/// Returned by `OllamaEmbeddingService::discover_dimension` so callers can
/// create a correctly-sized Qdrant collection without hardcoding a dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingModelInfo {
    /// The model name as configured (e.g. `"nomic-embed-text"` or `"qwen3-embedding:4b"`).
    pub model_name: String,
    /// The real vector dimension returned by the live model. Discovered from the
    /// actual embed response — not a hardcoded doc value.
    pub dimension: usize,
}

#[derive(Debug, Clone)]
pub struct OllamaEmbeddingConfig {
    pub base_url: String,
    pub model: String,
    pub max_concurrency: usize,
}

impl Default for OllamaEmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_owned(),
            model: DEFAULT_EMBEDDING_MODEL.to_owned(),
            max_concurrency: 4,
        }
    }
}

#[derive(Clone)]
pub struct OllamaEmbeddingService {
    client: reqwest::Client,
    config: OllamaEmbeddingConfig,
    semaphore: Arc<Semaphore>,
}

impl std::fmt::Debug for OllamaEmbeddingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OllamaEmbeddingService")
            .field("base_url", &self.config.base_url)
            .field("model", &self.config.model)
            .field("max_concurrency", &self.config.max_concurrency)
            .finish()
    }
}

impl OllamaEmbeddingService {
    pub fn from_config(config: OllamaEmbeddingConfig) -> Result<Self, EmbeddingError> {
        Self::new(reqwest::Client::new(), config)
    }

    pub fn new(
        client: reqwest::Client,
        config: OllamaEmbeddingConfig,
    ) -> Result<Self, EmbeddingError> {
        if config.model.trim().is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "embedding model must not be blank".to_owned(),
            ));
        }

        if config.base_url.trim().is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "embedding base_url must not be blank".to_owned(),
            ));
        }

        if config.max_concurrency == 0 {
            return Err(EmbeddingError::InvalidInput(
                "max_concurrency must be greater than zero".to_owned(),
            ));
        }

        Ok(Self {
            client,
            semaphore: Arc::new(Semaphore::new(config.max_concurrency)),
            config,
        })
    }

    /// Discovers the real embedding dimension by sending a minimal probe request
    /// to the live Ollama model and measuring the returned vector length.
    ///
    /// This must be called before `ensure_collection` so the collection is sized
    /// to the actual model output, not a hardcoded or doc-stated dimension. The
    /// qwen3-embedding:4b docs say 2560 but the live model is authoritative.
    ///
    /// Fails loud if the model returns an empty vector or is unreachable.
    pub async fn discover_dimension(&self) -> Result<EmbeddingModelInfo, EmbeddingError> {
        // A minimal ASCII probe that fits any embedding model's token budget.
        let probe_text = "dimension probe";
        let vector = self.embed(probe_text).await?;
        Ok(EmbeddingModelInfo {
            model_name: self.config.model.clone(),
            dimension: vector.len(),
        })
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "text input must not be blank".to_owned(),
            ));
        }

        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| EmbeddingError::Unexpected("embedding semaphore closed".to_owned()))?;

        let endpoint = format!(
            "{}/api/embeddings",
            self.config.base_url.trim_end_matches('/')
        );

        // Defense-in-depth window guard. An embedding input must fit the model's
        // single physical batch (no KV-cache streaming across batches like
        // generation): nomic-embed-text via Ollama rejects inputs over its 2048-
        // token batch with `HTTP 500: input (N tokens) is too large`. Callers are
        // expected to embed bounded text (skill summaries, individual subunits),
        // but a pathologically long description or one oversized subunit must never
        // take a whole service down. `cap_to_embed_window` TRUNCATES LOUDLY rather
        // than silently or by 500ing — truncation for a fixed embedding window is
        // correct, but it is surfaced, not hidden.
        let prompt = cap_to_embed_window(text);

        let request = EmbeddingsRequest {
            model: self.config.model.clone(),
            prompt,
        };

        let response = self
            .client
            .post(endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|error| EmbeddingError::ProviderUnavailable(error.to_string()))?;

        if response.status() != StatusCode::OK {
            return Err(EmbeddingError::ProviderUnavailable(format!(
                "ollama embedding endpoint returned {}",
                response.status()
            )));
        }

        let body: EmbeddingsResponse = response
            .json()
            .await
            .map_err(|error| EmbeddingError::Unexpected(error.to_string()))?;

        if body.embedding.is_empty() {
            return Err(EmbeddingError::Unexpected(
                "ollama embedding response returned an empty vector".to_owned(),
            ));
        }

        Ok(body.embedding)
    }
}

#[derive(Debug, Serialize)]
struct EmbeddingsRequest {
    model: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingService for OllamaEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed(text).await
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "batch input must contain at least one text".to_owned(),
            ));
        }

        let mut jobs = JoinSet::new();
        for (index, text) in texts.iter().enumerate() {
            let service = self.clone();
            let text = (*text).to_owned();
            jobs.spawn(async move {
                let vector = service.embed(&text).await;
                (index, vector)
            });
        }

        let mut ordered: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        while let Some(join_result) = jobs.join_next().await {
            let (index, vector_result) = match join_result {
                Ok(result) => result,
                Err(error) => {
                    jobs.abort_all();
                    return Err(EmbeddingError::Unexpected(format!(
                        "embedding batch task failed: {error}"
                    )));
                }
            };

            let vector = match vector_result {
                Ok(vector) => vector,
                Err(error) => {
                    jobs.abort_all();
                    return Err(error);
                }
            };
            ordered[index] = Some(vector);
        }

        ordered
            .into_iter()
            .map(|item| {
                item.ok_or_else(|| {
                    EmbeddingError::Unexpected(
                        "embedding batch result was missing an entry".to_owned(),
                    )
                })
            })
            .collect::<Result<Vec<Vec<f32>>, EmbeddingError>>()
    }
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
    };

    use super::*;

    /// Starts a one-shot HTTP server that responds to the first request with the
    /// supplied status and body. Returns the base URL and the server join handle.
    async fn spawn_response_server(status_line: &str, body: &str) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("bound listener should have local addr");
        let status_line = status_line.to_owned();
        let body = body.to_owned();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("server should accept one connection");
            let mut buf = vec![0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("server should write response");
        });

        (format!("http://{address}"), server)
    }

    /// Proves `discover_dimension` returns the real vector length from the live
    /// embed response — NOT a hardcoded value.
    #[tokio::test]
    async fn discover_dimension_returns_real_vector_length_from_embed_response() {
        // Fake Ollama: returns a 5-dim vector (arbitrary test size).
        let fake_vector = vec![0.1_f32; 5];
        let body = format!(
            r#"{{"embedding":{}}}"#,
            serde_json::to_string(&fake_vector).unwrap()
        );
        let (base_url, server) = spawn_response_server("200 OK", &body).await;

        let config = OllamaEmbeddingConfig {
            base_url,
            model: "test-model".to_owned(),
            max_concurrency: 1,
        };
        let service =
            OllamaEmbeddingService::new(reqwest::Client::new(), config).expect("config valid");

        let info = service
            .discover_dimension()
            .await
            .expect("discover_dimension must succeed with a valid embed response");

        assert_eq!(info.model_name, "test-model");
        assert_eq!(
            info.dimension, 5,
            "dimension must match the live response length"
        );

        server.await.expect("mock server should complete");
    }

    /// Proves `EmbeddingModelInfo` derives `Clone` and `PartialEq` correctly —
    /// a clone equals the original and two distinct instances with the same fields
    /// are equal, while differing instances are not.
    #[test]
    fn embedding_model_info_clone_and_eq_round_trip() {
        let original = EmbeddingModelInfo {
            model_name: "qwen3-embedding:4b".to_owned(),
            dimension: 2560,
        };
        let cloned = original.clone();
        assert_eq!(original, cloned, "clone must equal the original");

        let other = EmbeddingModelInfo {
            model_name: "nomic-embed-text".to_owned(),
            dimension: 768,
        };
        assert_ne!(
            original, other,
            "instances with different fields must not be equal"
        );
    }

    /// Proves `resolve_embedding_model` returns the default when the raw value is None.
    #[test]
    fn resolve_embedding_model_returns_default_when_raw_is_none() {
        assert_eq!(
            resolve_embedding_model(None),
            "qwen3-embedding:4b",
            "None (env var unset) must yield the qwen3 default"
        );
    }

    /// Proves `resolve_embedding_model` returns the default when the raw value is blank.
    #[test]
    fn resolve_embedding_model_returns_default_when_raw_is_blank() {
        assert_eq!(
            resolve_embedding_model(Some("")),
            "qwen3-embedding:4b",
            "empty string (docker-compose interpolation) must yield the qwen3 default"
        );
        assert_eq!(
            resolve_embedding_model(Some("   ")),
            "qwen3-embedding:4b",
            "whitespace-only string must yield the qwen3 default"
        );
    }

    /// Proves `resolve_embedding_model` returns the configured model when the raw value is set.
    #[test]
    fn resolve_embedding_model_returns_configured_model_when_raw_is_set() {
        assert_eq!(
            resolve_embedding_model(Some("qwen3-embedding:4b")),
            "qwen3-embedding:4b",
            "a non-blank model name must be returned as-is"
        );
    }

    #[tokio::test]
    async fn embed_text_rejects_blank_input() {
        let client = reqwest::Client::new();
        let service = OllamaEmbeddingService::new(client, OllamaEmbeddingConfig::default())
            .expect("default config should be valid");

        let error = service
            .embed_text("   ")
            .await
            .expect_err("blank input must fail");

        assert!(matches!(error, EmbeddingError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn embed_batch_rejects_empty_batch() {
        let client = reqwest::Client::new();
        let service = OllamaEmbeddingService::new(client, OllamaEmbeddingConfig::default())
            .expect("default config should be valid");

        let error = service
            .embed_batch(&[])
            .await
            .expect_err("empty batch must fail");

        assert!(matches!(error, EmbeddingError::InvalidInput(_)));
    }

    #[test]
    fn cap_to_embed_window_passes_short_input_through_unchanged() {
        let text = "name description tag-a tag-b";
        assert_eq!(cap_to_embed_window(text), text);
    }

    #[test]
    fn cap_to_embed_window_truncates_oversized_input_to_the_limit() {
        let text = "x".repeat(MAX_EMBED_INPUT_CHARS + 500);
        let capped = cap_to_embed_window(&text);
        assert_eq!(capped.chars().count(), MAX_EMBED_INPUT_CHARS);
    }

    #[test]
    fn cap_to_embed_window_truncates_on_a_char_boundary_for_multibyte_input() {
        // A string of multi-byte chars whose count exceeds the limit. Truncation
        // must produce a valid UTF-8 string capped at MAX_EMBED_INPUT_CHARS chars
        // (never panic on a byte-slice mid-codepoint).
        let text = "é".repeat(MAX_EMBED_INPUT_CHARS + 10);
        let capped = cap_to_embed_window(&text);
        assert_eq!(capped.chars().count(), MAX_EMBED_INPUT_CHARS);
        assert!(capped.chars().all(|c| c == 'é'));
    }
}
