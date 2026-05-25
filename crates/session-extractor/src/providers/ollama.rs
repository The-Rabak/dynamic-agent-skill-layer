use std::sync::Arc;

use domain::{ExtractionError, TranscriptSkillExtractionService};
use infrastructure::{OllamaExtractionConfig, OllamaExtractor};

/// Builds the Ollama-backed extraction adapter using infrastructure defaults and env overrides.
pub fn build_extractor(
    client: reqwest::Client,
) -> Result<Arc<dyn TranscriptSkillExtractionService>, ExtractionError> {
    let mut config = OllamaExtractionConfig::default();
    if let Ok(endpoint) = std::env::var("OLLAMA_EXTRACTION_ENDPOINT") {
        config.endpoint = endpoint;
    }
    if let Ok(model) = std::env::var("OLLAMA_EXTRACTION_MODEL") {
        config.model = model;
    }
    if let Ok(timeout_ms) = std::env::var("OLLAMA_EXTRACTION_TIMEOUT_MS") {
        config.timeout_ms = timeout_ms.parse().map_err(|error| {
            ExtractionError::InvalidTranscript(format!(
                "invalid OLLAMA_EXTRACTION_TIMEOUT_MS value: {error}"
            ))
        })?;
    }

    OllamaExtractor::new(client, config)
        .map(|extractor| Arc::new(extractor) as Arc<dyn TranscriptSkillExtractionService>)
}
