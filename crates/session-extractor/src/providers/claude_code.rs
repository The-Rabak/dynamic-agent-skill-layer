use std::sync::Arc;

use domain::{ExtractionError, TranscriptSkillExtractionService};
use infrastructure::{ClaudeCodeExtractionConfig, ClaudeCodeExtractor};

/// Builds the Claude Code CLI-backed extraction adapter (subscription-based, no API key).
///
/// Reads opt-in provider configuration from the environment:
/// - `CLAUDE_CLI_PATH` (default `claude` — resolved via `$PATH`)
/// - `EXTRACT_SESSION_MODEL` (default `claude-sonnet-4-6`)
/// - `CLAUDE_CODE_EXTRACTION_TIMEOUT_MS` (optional inner timeout override; default 120 000 ms)
///
/// **Host-only constraint:** The `claude` CLI authenticates via the user's Claude Code
/// subscription on the host machine. This provider is NOT suitable for Docker/container
/// deployments where the host CLI binary is unavailable. In containerised environments use
/// `EXTRACT_SESSION_PROVIDER=claude-api` (Anthropic Messages API + API key) or
/// `EXTRACT_SESSION_PROVIDER=ollama` (local; the compose default).
///
/// No `ANTHROPIC_API_KEY` is read or required for this provider.
pub fn build_extractor() -> Result<Arc<dyn TranscriptSkillExtractionService>, ExtractionError> {
    let mut config = ClaudeCodeExtractionConfig::default();

    if let Ok(cli_path) = std::env::var("CLAUDE_CLI_PATH")
        && !cli_path.trim().is_empty()
    {
        config.cli_path = cli_path;
    }
    if let Ok(model) = std::env::var("EXTRACT_SESSION_MODEL")
        && !model.trim().is_empty()
    {
        config.model = model;
    }
    if let Ok(timeout_str) = std::env::var("CLAUDE_CODE_EXTRACTION_TIMEOUT_MS") {
        config.timeout_ms = timeout_str.parse().map_err(|error| {
            ExtractionError::InvalidTranscript(format!(
                "invalid CLAUDE_CODE_EXTRACTION_TIMEOUT_MS value: {error}"
            ))
        })?;
    }

    ClaudeCodeExtractor::new(config)
        .map(|extractor| Arc::new(extractor) as Arc<dyn TranscriptSkillExtractionService>)
}
