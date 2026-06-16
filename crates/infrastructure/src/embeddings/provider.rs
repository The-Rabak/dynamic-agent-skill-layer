//! Embedding-provider selection: one place that turns env config into the live
//! embedding service, so every production process (mcp-server, graph-builder,
//! maintenance, session-extractor) picks the SAME provider and never writes one
//! provider's vectors into another's model-keyed collection.
//!
//! Selected by `EMBEDDING_PROVIDER` (default `ollama`):
//! - `ollama` → [`OllamaEmbeddingService`] from `OLLAMA_URL` + `OLLAMA_EMBED_MODEL`.
//! - `tei`    → [`TeiEmbeddingService`] from `TEI_URL`; the arm label still comes
//!   from `OLLAMA_EMBED_MODEL` (re-used as the collection/health identity — set it
//!   to e.g. `qwen3-embedding-4b-tei` to keep the TEI collection isolated).
//!
//! Two front doors over ONE selection function:
//! - [`DynEmbeddingService::from_env`] returns the concrete provider enum, for the
//!   one caller (mcp-server) whose `RetrievalOrchestrator<E>` is generic over a
//!   concrete `EmbeddingService` type and so cannot take a bare trait object.
//! - [`build_embedding_service_from_env`] wraps the same enum as `Arc<dyn
//!   EmbeddingService>` for the callers that only need the trait object.
//!
//! Missing required config fails loud (no silent fallback to a different provider).

use std::sync::Arc;

use async_trait::async_trait;
use domain::{EmbeddingError, EmbeddingService};

use crate::embeddings::{
    ollama::{
        EmbeddingModelInfo, OllamaEmbeddingConfig, OllamaEmbeddingService, embedding_model_from_env,
    },
    tei::{TeiEmbeddingConfig, TeiEmbeddingService, tei_client_batch_size_from_env},
};

/// Reads `EMBEDDING_PROVIDER`, normalised to lowercase. Unset or blank → `ollama`.
pub fn embedding_provider_from_env() -> String {
    std::env::var("EMBEDDING_PROVIDER")
        .ok()
        .map(|raw| raw.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "ollama".to_owned())
}

/// A concrete `EmbeddingService` that dispatches to the configured provider.
///
/// Exists so a single concrete type can be used as the generic parameter `E` in
/// `RetrievalOrchestrator<E>` while still supporting provider selection at boot.
/// Method dispatch is a cheap enum match per call.
#[derive(Debug, Clone)]
pub enum DynEmbeddingService {
    Ollama(OllamaEmbeddingService),
    Tei(TeiEmbeddingService),
}

impl DynEmbeddingService {
    /// Builds the live embedding service for the configured provider.
    ///
    /// `max_concurrency` is the per-process embed concurrency cap (callers source
    /// it from their own env, e.g. mcp-server's `EMBED_MAX_CONCURRENCY`). Fails loud
    /// when the selected provider's required URL env var is unset, or when the
    /// provider name is unknown — there is NO fallback to a different provider.
    pub fn from_env(max_concurrency: usize) -> Result<Self, EmbeddingError> {
        let provider = embedding_provider_from_env();
        match provider.as_str() {
            "ollama" => {
                let base_url = std::env::var("OLLAMA_URL").map_err(|_| {
                    EmbeddingError::InvalidInput(
                        "OLLAMA_URL must be set for EMBEDDING_PROVIDER=ollama".to_owned(),
                    )
                })?;
                let config = OllamaEmbeddingConfig {
                    base_url,
                    model: embedding_model_from_env(),
                    max_concurrency,
                };
                Ok(Self::Ollama(OllamaEmbeddingService::from_config(config)?))
            }
            "tei" => {
                let base_url = std::env::var("TEI_URL").map_err(|_| {
                    EmbeddingError::InvalidInput(
                        "TEI_URL must be set for EMBEDDING_PROVIDER=tei".to_owned(),
                    )
                })?;
                let config = TeiEmbeddingConfig {
                    base_url,
                    // Re-use the embed-model env as the arm identity for collection
                    // naming + /health; TEI itself serves the model fixed at launch.
                    model_label: embedding_model_from_env(),
                    max_concurrency,
                    client_batch_size: tei_client_batch_size_from_env(),
                };
                Ok(Self::Tei(TeiEmbeddingService::from_config(config)?))
            }
            other => Err(EmbeddingError::InvalidInput(format!(
                "unknown EMBEDDING_PROVIDER={other:?}; expected 'ollama' or 'tei'"
            ))),
        }
    }
}

#[async_trait]
impl EmbeddingService for DynEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        match self {
            Self::Ollama(service) => service.embed_text(text).await,
            Self::Tei(service) => service.embed_text(text).await,
        }
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        match self {
            Self::Ollama(service) => service.embed_batch(texts).await,
            Self::Tei(service) => service.embed_batch(texts).await,
        }
    }
}

/// Builds the live embedding service as a trait object for callers that do not
/// need the concrete provider type. Thin wrapper over [`DynEmbeddingService::from_env`].
pub fn build_embedding_service_from_env(
    max_concurrency: usize,
) -> Result<Arc<dyn EmbeddingService>, EmbeddingError> {
    Ok(Arc::new(DynEmbeddingService::from_env(max_concurrency)?) as Arc<dyn EmbeddingService>)
}

/// Provider-agnostic embedding-arm discovery: probes the live service for its real
/// vector dimension (never hardcoded) and pairs it with the arm identity from
/// `OLLAMA_EMBED_MODEL`. Works through the trait object, so it serves both the
/// Ollama and TEI providers (replacing the Ollama-only inherent `discover_dimension`
/// at the production call sites).
pub async fn discover_embedding_arm(
    service: &dyn EmbeddingService,
) -> Result<EmbeddingModelInfo, EmbeddingError> {
    let probe = service.embed_text("dimension probe").await?;
    if probe.is_empty() {
        return Err(EmbeddingError::Unexpected(
            "embedding dimension probe returned an empty vector".to_owned(),
        ));
    }
    Ok(EmbeddingModelInfo {
        model_name: embedding_model_from_env(),
        dimension: probe.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `EMBEDDING_PROVIDER` unset resolves to the Ollama default. Exercised without
    /// leaving global state mutated; the env-dependent provider branches are covered
    /// by the live e2e arms.
    #[test]
    fn default_provider_is_ollama_when_unset() {
        // SAFETY: single-threaded test; the prior value is restored before return.
        let prior = std::env::var("EMBEDDING_PROVIDER").ok();
        unsafe { std::env::remove_var("EMBEDDING_PROVIDER") };
        assert_eq!(embedding_provider_from_env(), "ollama");
        if let Some(value) = prior {
            unsafe { std::env::set_var("EMBEDDING_PROVIDER", value) };
        }
    }
}
