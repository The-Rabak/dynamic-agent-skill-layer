use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use domain::{EmbeddingError, EmbeddingService};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::{sync::Semaphore, task::JoinSet, time::timeout};

#[derive(Debug, Clone)]
pub struct OllamaEmbeddingConfig {
    pub base_url: String,
    pub model: String,
    pub timeout_ms: u64,
    pub batch_timeout_ms: u64,
    pub max_concurrency: usize,
}

impl Default for OllamaEmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".to_owned(),
            model: "nomic-embed-text".to_owned(),
            timeout_ms: 500,
            batch_timeout_ms: 5_000,
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
            .field("timeout_ms", &self.config.timeout_ms)
            .field("batch_timeout_ms", &self.config.batch_timeout_ms)
            .field("max_concurrency", &self.config.max_concurrency)
            .finish()
    }
}

impl OllamaEmbeddingService {
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

        if config.timeout_ms == 0 || config.batch_timeout_ms == 0 {
            return Err(EmbeddingError::InvalidInput(
                "embedding timeouts must be greater than zero".to_owned(),
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

    async fn embed_with_timeout(
        &self,
        text: &str,
        timeout_ms: u64,
    ) -> Result<Vec<f32>, EmbeddingError> {
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
        let request = EmbeddingsRequest {
            model: self.config.model.clone(),
            prompt: text.to_owned(),
        };

        timeout(Duration::from_millis(timeout_ms), async {
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
        })
        .await
        .map_err(|_| EmbeddingError::Timeout { timeout_ms })
        .and_then(|result| result)
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
        self.embed_with_timeout(text, self.config.timeout_ms).await
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Err(EmbeddingError::InvalidInput(
                "batch input must contain at least one text".to_owned(),
            ));
        }

        timeout(Duration::from_millis(self.config.batch_timeout_ms), async {
            let mut jobs = JoinSet::new();
            for (index, text) in texts.iter().enumerate() {
                let service = self.clone();
                let text = (*text).to_owned();
                jobs.spawn(async move {
                    let vector = service
                        .embed_with_timeout(&text, service.config.timeout_ms)
                        .await;
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
        })
        .await
        .map_err(|_| EmbeddingError::Timeout {
            timeout_ms: self.config.batch_timeout_ms,
        })
        .and_then(|result| result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
