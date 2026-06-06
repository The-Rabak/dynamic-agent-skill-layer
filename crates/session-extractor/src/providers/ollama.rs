use std::sync::Arc;

use domain::{ExtractionError, TranscriptSkillExtractionService};
use infrastructure::{OllamaExtractionConfig, OllamaExtractor};

/// Builds the Ollama-backed extraction adapter using infrastructure defaults and env overrides.
///
/// Recognized environment variables:
/// - `OLLAMA_EXTRACTION_ENDPOINT`: full URL to Ollama's `/api/generate` endpoint
/// - `OLLAMA_EXTRACTION_MODEL`: model name to use for extraction (e.g. `gemma4:12b`)
/// - `OLLAMA_EXTRACTION_TEMPERATURE`: sampling temperature float 0.0–2.0; omit to use
///   the model default. Set to `0` for deterministic (greedy) output in e2e tests.
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
    if let Ok(temperature_str) = std::env::var("OLLAMA_EXTRACTION_TEMPERATURE") {
        let temperature: f32 = temperature_str.parse().map_err(|error| {
            ExtractionError::InvalidTranscript(format!(
                "invalid OLLAMA_EXTRACTION_TEMPERATURE value: {error}"
            ))
        })?;
        config.temperature = Some(temperature);
    }

    OllamaExtractor::new(client, config)
        .map(|extractor| Arc::new(extractor) as Arc<dyn TranscriptSkillExtractionService>)
}
