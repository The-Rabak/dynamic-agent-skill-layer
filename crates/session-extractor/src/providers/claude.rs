use std::sync::Arc;

use domain::{ExtractionError, TranscriptSkillExtractionService};
use infrastructure::{ClaudeExtractionConfig, ClaudeExtractor};

/// Builds the Claude-backed extraction adapter (direct Anthropic Messages API).
///
/// Selected via `EXTRACT_SESSION_PROVIDER=claude-api`. Requires `ANTHROPIC_API_KEY`.
/// For subscription-based extraction without an API key, use
/// `EXTRACT_SESSION_PROVIDER=claude` (the `claude_code` provider).
///
/// Reads opt-in provider configuration from the environment:
/// - `ANTHROPIC_API_KEY` (required — missing key fails loudly at construct time)
/// - `EXTRACT_SESSION_MODEL` (default `claude-sonnet-4-6`)
/// - `ANTHROPIC_BASE_URL` (default `https://api.anthropic.com`)
/// - `CLAUDE_EXTRACTION_TIMEOUT_MS` (optional inner timeout override)
///
/// The API key is read from the environment and never committed.
pub fn build_extractor(
    client: reqwest::Client,
) -> Result<Arc<dyn TranscriptSkillExtractionService>, ExtractionError> {
    let mut config = ClaudeExtractionConfig::default();

    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        config.api_key = api_key;
    }
    if let Ok(base_url) = std::env::var("ANTHROPIC_BASE_URL")
        && !base_url.trim().is_empty()
    {
        config.base_url = base_url;
    }
    if let Ok(model) = std::env::var("EXTRACT_SESSION_MODEL")
        && !model.trim().is_empty()
    {
        config.model = model;
    }
    if let Ok(timeout_ms) = std::env::var("CLAUDE_EXTRACTION_TIMEOUT_MS") {
        config.timeout_ms = timeout_ms.parse().map_err(|error| {
            ExtractionError::InvalidTranscript(format!(
                "invalid CLAUDE_EXTRACTION_TIMEOUT_MS value: {error}"
            ))
        })?;
    }

    ClaudeExtractor::new(client, config)
        .map(|extractor| Arc::new(extractor) as Arc<dyn TranscriptSkillExtractionService>)
}
