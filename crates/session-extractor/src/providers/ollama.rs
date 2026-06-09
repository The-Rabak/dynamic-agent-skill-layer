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
    apply_transcript_limit_overrides(
        &mut config.max_entries,
        &mut config.max_entry_chars,
        &mut config.max_total_chars,
    )?;

    OllamaExtractor::new(client, config)
        .map(|extractor| Arc::new(extractor) as Arc<dyn TranscriptSkillExtractionService>)
}

/// Applies the provider-agnostic transcript-parser limit overrides from the
/// environment onto an extraction config, in place. Shared by every provider so
/// the reality-sized ceilings (see the config defaults) stay tunable rather than
/// hardcoded. Recognized (all optional, fail-loud on a non-integer value):
/// - `EXTRACT_MAX_ENTRIES`
/// - `EXTRACT_MAX_ENTRY_CHARS`
/// - `EXTRACT_MAX_TOTAL_CHARS`
pub(crate) fn apply_transcript_limit_overrides(
    max_entries: &mut usize,
    max_entry_chars: &mut usize,
    max_total_chars: &mut usize,
) -> Result<(), ExtractionError> {
    for (var, slot) in [
        ("EXTRACT_MAX_ENTRIES", max_entries),
        ("EXTRACT_MAX_ENTRY_CHARS", max_entry_chars),
        ("EXTRACT_MAX_TOTAL_CHARS", max_total_chars),
    ] {
        if let Ok(raw) = std::env::var(var)
            && !raw.trim().is_empty()
        {
            *slot = raw.trim().parse().map_err(|error| {
                ExtractionError::InvalidTranscript(format!("invalid {var} value: {error}"))
            })?;
        }
    }
    Ok(())
}
