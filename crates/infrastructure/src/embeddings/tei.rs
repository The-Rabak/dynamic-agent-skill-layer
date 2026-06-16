//! TEI (HuggingFace Text Embeddings Inference) embedding service.
//!
//! A second [`EmbeddingService`] implementation, parity-matched to
//! [`crate::embeddings::ollama::OllamaEmbeddingService`], used by the V1.7
//! embedding-model/server A/B experiment (arms A1 = `4b`/TEI, A3 = `0.6b`/TEI).
//!
//! TEI serves a SINGLE model fixed at container launch (`--model-id`), so unlike
//! the Ollama path this client sends no model field per request — the `model_label`
//! here is only the *arm identity* used for the model-keyed Qdrant collection name
//! and the `/health` `embedding_arm` line (e.g. `qwen3-embedding-4b-tei`), kept
//! distinct from the Ollama collection of the "same" weights so vectors never mix.
//!
//! Parity with the Ollama path (so the A/B compares the model/server, not our
//! client behaviour):
//! - **Raw text, no instruction prefix.** The Ollama `/api/embeddings` path embeds
//!   the bare text with no `Instruct:`/`Query:` prefix, so this client never sends
//!   TEI's `prompt_name`. (A pooling/prefix drift surfaces as the A1 ±0.02 parity
//!   miss — that is the experiment's diagnostic, not a thing to paper over.)
//! - **Same input window.** Inputs are capped by the shared
//!   [`crate::embeddings::ollama::cap_to_embed_window`] before the request.
//! - **Cosine ranking.** The Qdrant collections use Cosine distance, which is
//!   scale-invariant, so `normalize: true` (TEI's default) does not perturb
//!   ranking relative to Ollama.
//!
//! The win this client exists for: TEI's batch `/embed` accepts many inputs in one
//! request, so [`TeiEmbeddingService::embed_batch`] issues ONE HTTP call per
//! client-batch chunk instead of the Ollama path's one call per text — the
//! multi-view priming latency lever the experiment is measuring.

use std::sync::Arc;

use async_trait::async_trait;
use domain::{EmbeddingError, EmbeddingService};
use reqwest::StatusCode;
use serde::Serialize;
use tokio::{sync::Semaphore, task::JoinSet};

use crate::embeddings::ollama::cap_to_embed_window;

/// Default number of inputs sent in a single TEI `/embed` request.
///
/// TEI rejects client batches larger than its `--max-client-batch-size` (default
/// 32). We chunk `embed_batch` into requests of this size and run the chunks
/// concurrently (bounded by `max_concurrency`). Tunable via `TEI_CLIENT_BATCH_SIZE`.
pub const DEFAULT_TEI_CLIENT_BATCH_SIZE: usize = 32;

/// Reads the TEI client batch size from `TEI_CLIENT_BATCH_SIZE`, falling back to
/// [`DEFAULT_TEI_CLIENT_BATCH_SIZE`]. A blank or unparseable value falls back with
/// a `warn!` rather than failing — this is a throughput knob, not correctness.
pub fn tei_client_batch_size_from_env() -> usize {
    match std::env::var("TEI_CLIENT_BATCH_SIZE") {
        Ok(raw) if !raw.trim().is_empty() => match raw.trim().parse::<usize>() {
            Ok(value) if value > 0 => value,
            _ => {
                tracing::warn!(
                    raw = %raw,
                    default = DEFAULT_TEI_CLIENT_BATCH_SIZE,
                    "TEI_CLIENT_BATCH_SIZE is set but not a positive integer; using default"
                );
                DEFAULT_TEI_CLIENT_BATCH_SIZE
            }
        },
        _ => DEFAULT_TEI_CLIENT_BATCH_SIZE,
    }
}

#[derive(Debug, Clone)]
pub struct TeiEmbeddingConfig {
    /// Base URL of the TEI server, e.g. `http://127.0.0.1:8085`.
    pub base_url: String,
    /// Arm-identity label used ONLY for collection naming + `/health` reporting.
    /// Not sent to TEI (which serves one fixed model). E.g. `qwen3-embedding-4b-tei`.
    pub model_label: String,
    /// Max concurrent in-flight `/embed` requests.
    pub max_concurrency: usize,
    /// Max inputs per `/embed` request (must be ≤ TEI `--max-client-batch-size`).
    pub client_batch_size: usize,
}

impl Default for TeiEmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8085".to_owned(),
            model_label: "tei-embedding".to_owned(),
            max_concurrency: 4,
            client_batch_size: DEFAULT_TEI_CLIENT_BATCH_SIZE,
        }
    }
}

#[derive(Clone)]
pub struct TeiEmbeddingService {
    client: reqwest::Client,
    config: TeiEmbeddingConfig,
    semaphore: Arc<Semaphore>,
}

impl std::fmt::Debug for TeiEmbeddingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeiEmbeddingService")
            .field("base_url", &self.config.base_url)
            .field("model_label", &self.config.model_label)
            .field("max_concurrency", &self.config.max_concurrency)
            .field("client_batch_size", &self.config.client_batch_size)
            .finish()
    }
}

impl TeiEmbeddingService {
    pub fn from_config(config: TeiEmbeddingConfig) -> Result<Self, EmbeddingError> {
        Self::new(reqwest::Client::new(), config)
    }

    pub fn new(
        client: reqwest::Client,
        config: TeiEmbeddingConfig,
    ) -> Result<Self, EmbeddingError> {
        if config.base_url.trim().is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "TEI base_url must not be blank".to_owned(),
            ));
        }
        if config.model_label.trim().is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "TEI model_label must not be blank".to_owned(),
            ));
        }
        if config.max_concurrency == 0 {
            return Err(EmbeddingError::InvalidInput(
                "max_concurrency must be greater than zero".to_owned(),
            ));
        }
        if config.client_batch_size == 0 {
            return Err(EmbeddingError::InvalidInput(
                "client_batch_size must be greater than zero".to_owned(),
            ));
        }

        Ok(Self {
            client,
            semaphore: Arc::new(Semaphore::new(config.max_concurrency)),
            config,
        })
    }

    /// Sends one `/embed` request for an already-capped, already-validated chunk of
    /// inputs and returns one vector per input, in input order. Holds a semaphore
    /// permit for the duration to bound concurrent requests.
    async fn embed_chunk(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| EmbeddingError::Unexpected("embedding semaphore closed".to_owned()))?;

        let endpoint = format!("{}/embed", self.config.base_url.trim_end_matches('/'));

        let request = TeiEmbedRequest {
            inputs: inputs.iter().map(String::as_str).collect(),
            // No `prompt_name`: parity with the Ollama path (raw text, no instruction).
            normalize: true,
            // Server-side safety net on top of our char cap; the model's token
            // window is authoritative for the final truncation.
            truncate: true,
        };

        let response = self
            .client
            .post(endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|error| EmbeddingError::ProviderUnavailable(error.to_string()))?;

        if response.status() != StatusCode::OK {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(EmbeddingError::ProviderUnavailable(format!(
                "TEI embedding endpoint returned {status}: {body}"
            )));
        }

        // TEI `/embed` returns a bare JSON array of float arrays, one per input.
        let vectors: Vec<Vec<f32>> = response
            .json()
            .await
            .map_err(|error| EmbeddingError::Unexpected(error.to_string()))?;

        if vectors.len() != inputs.len() {
            return Err(EmbeddingError::Unexpected(format!(
                "TEI returned {} vectors for {} inputs",
                vectors.len(),
                inputs.len()
            )));
        }
        if vectors.iter().any(Vec::is_empty) {
            return Err(EmbeddingError::Unexpected(
                "TEI embedding response contained an empty vector".to_owned(),
            ));
        }

        Ok(vectors)
    }
}

#[derive(Debug, Serialize)]
struct TeiEmbedRequest<'a> {
    inputs: Vec<&'a str>,
    normalize: bool,
    truncate: bool,
}

#[async_trait]
impl EmbeddingService for TeiEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "text input must not be blank".to_owned(),
            ));
        }
        let capped = cap_to_embed_window(text);
        let mut vectors = self.embed_chunk(&[capped]).await?;
        vectors.pop().ok_or_else(|| {
            EmbeddingError::Unexpected("TEI returned no vector for a single input".to_owned())
        })
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "batch input must contain at least one text".to_owned(),
            ));
        }
        if texts.iter().any(|t| t.trim().is_empty()) {
            return Err(EmbeddingError::InvalidInput(
                "batch input must not contain a blank text".to_owned(),
            ));
        }

        // Chunk into client-batch-sized requests and run them concurrently. Each
        // chunk is one HTTP `/embed` call (the batching latency win); chunks are
        // re-assembled in original order.
        let mut jobs = JoinSet::new();
        for (chunk_index, chunk) in texts.chunks(self.config.client_batch_size).enumerate() {
            let inputs: Vec<String> = chunk.iter().map(|t| cap_to_embed_window(t)).collect();
            let service = self.clone();
            jobs.spawn(async move {
                let result = service.embed_chunk(&inputs).await;
                (chunk_index, result)
            });
        }

        let chunk_count = texts.len().div_ceil(self.config.client_batch_size);
        let mut ordered: Vec<Option<Vec<Vec<f32>>>> = (0..chunk_count).map(|_| None).collect();
        while let Some(join_result) = jobs.join_next().await {
            let (chunk_index, chunk_result) = match join_result {
                Ok(result) => result,
                Err(error) => {
                    jobs.abort_all();
                    return Err(EmbeddingError::Unexpected(format!(
                        "embedding batch task failed: {error}"
                    )));
                }
            };
            match chunk_result {
                Ok(vectors) => ordered[chunk_index] = Some(vectors),
                Err(error) => {
                    jobs.abort_all();
                    return Err(error);
                }
            }
        }

        let mut flattened = Vec::with_capacity(texts.len());
        for (chunk_index, slot) in ordered.into_iter().enumerate() {
            let vectors = slot.ok_or_else(|| {
                EmbeddingError::Unexpected(format!(
                    "embedding batch result was missing chunk {chunk_index}"
                ))
            })?;
            flattened.extend(vectors);
        }
        Ok(flattened)
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

    /// Starts a one-shot HTTP server that replies to the first request with the
    /// supplied status and body. Returns the base URL and the join handle.
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
            let mut buf = vec![0u8; 8192];
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

    fn service_for(base_url: String) -> TeiEmbeddingService {
        TeiEmbeddingService::new(
            reqwest::Client::new(),
            TeiEmbeddingConfig {
                base_url,
                model_label: "qwen3-embedding-4b-tei".to_owned(),
                max_concurrency: 2,
                client_batch_size: 32,
            },
        )
        .expect("config valid")
    }

    #[tokio::test]
    async fn embed_text_parses_first_vector_from_tei_array_response() {
        let body = r#"[[0.1,0.2,0.3,0.4]]"#;
        let (base_url, server) = spawn_response_server("200 OK", body).await;
        let service = service_for(base_url);

        let vector = service
            .embed_text("hello")
            .await
            .expect("embed_text must parse the TEI array-of-arrays response");

        assert_eq!(vector, vec![0.1_f32, 0.2, 0.3, 0.4]);
        server.await.expect("mock server should complete");
    }

    #[tokio::test]
    async fn embed_text_rejects_blank_input() {
        let service = service_for("http://127.0.0.1:0".to_owned());
        let error = service
            .embed_text("   ")
            .await
            .expect_err("blank input must fail before any request");
        assert!(matches!(error, EmbeddingError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn embed_batch_rejects_empty_batch() {
        let service = service_for("http://127.0.0.1:0".to_owned());
        let error = service
            .embed_batch(&[])
            .await
            .expect_err("empty batch must fail");
        assert!(matches!(error, EmbeddingError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn embed_batch_rejects_blank_member() {
        let service = service_for("http://127.0.0.1:0".to_owned());
        let error = service
            .embed_batch(&["ok", "   "])
            .await
            .expect_err("a blank batch member must fail");
        assert!(matches!(error, EmbeddingError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn embed_chunk_errors_when_vector_count_mismatches_inputs() {
        // Two inputs but the server returns one vector → must fail loud, not silently drop.
        let body = r#"[[0.1,0.2]]"#;
        let (base_url, server) = spawn_response_server("200 OK", body).await;
        let service = service_for(base_url);

        let error = service
            .embed_batch(&["a", "b"])
            .await
            .expect_err("count mismatch must fail");
        assert!(matches!(error, EmbeddingError::Unexpected(_)));
        server.await.expect("mock server should complete");
    }

    #[tokio::test]
    async fn embed_text_surfaces_non_200_as_provider_unavailable() {
        let (base_url, server) =
            spawn_response_server("503 Service Unavailable", "overloaded").await;
        let service = service_for(base_url);

        let error = service
            .embed_text("hello")
            .await
            .expect_err("non-200 must surface as provider-unavailable");
        assert!(matches!(error, EmbeddingError::ProviderUnavailable(_)));
        server.await.expect("mock server should complete");
    }
}
