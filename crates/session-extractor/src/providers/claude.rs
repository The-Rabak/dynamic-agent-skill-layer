use std::sync::Arc;

use domain::{ExtractionError, TranscriptSkillExtractionService};
use infrastructure::{ClaudeExtractionConfig, ClaudeExtractor};

/// Builds the Claude-backed extraction adapter (direct Anthropic Messages API).
///
/// Selected via `EXTRACT_SESSION_PROVIDER=claude` (or the accepted alias `=claude-api`).
/// Requires `ANTHROPIC_API_KEY` — missing key fails loudly at construct time
/// (Constitution Principle 1: no silent cloud attempt, no silent fallback).
/// For subscription-based extraction without an API key, use
/// `EXTRACT_SESSION_PROVIDER=claude-code` (the `claude_code` provider).
///
/// Reads opt-in provider configuration from the environment:
/// - `ANTHROPIC_API_KEY` (required — missing key fails loudly at construct time)
/// - `EXTRACT_SESSION_MODEL` (default `claude-sonnet-4-6`)
/// - `ANTHROPIC_BASE_URL` (default `https://api.anthropic.com`)
/// - `CLAUDE_EXTRACTION_TIMEOUT_MS` (optional inner timeout override)
///
/// The API key is read from the environment and never committed.
///
/// **Security — `ANTHROPIC_BASE_URL` is operator-controlled infra config:**
/// Operator-supplied base URLs are validated to use the `https://` scheme.
/// Allowing arbitrary `http://` URLs is an exfiltration risk: extraction transcripts
/// (which may contain secrets or proprietary code) are POSTed to this endpoint. Only
/// `https://` is accepted in production. Non-https values are rejected at construction
/// with `ExtractionError::ProviderUnavailable`. (Note for #126 docs: document this
/// validation and the operator-only exfiltration warning in capability-catalog.md.)
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
        validate_base_url_scheme(&base_url)?;
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

/// Validates that an operator-supplied `ANTHROPIC_BASE_URL` uses the `https://` scheme.
///
/// Extraction transcripts may contain secrets or proprietary code. Allowing arbitrary
/// `http://` base URLs would let an attacker who can set `ANTHROPIC_BASE_URL` exfiltrate
/// transcript content in plaintext. Only `https://` is accepted in production.
///
/// `http://` is allowed only when the `test` cfg is active (e.g. `http://127.0.0.1`
/// for unit tests that exercise connection-error paths without a real HTTPS server).
fn validate_base_url_scheme(url: &str) -> Result<(), ExtractionError> {
    let trimmed = url.trim();
    if trimmed.starts_with("https://") {
        return Ok(());
    }

    // Allow http:// only in test builds. This gate prevents leaking plaintext
    // transcripts in production while still letting unit tests point at a local
    // stub port without a TLS certificate.
    #[cfg(test)]
    if trimmed.starts_with("http://") {
        return Ok(());
    }

    Err(ExtractionError::ProviderUnavailable(format!(
        "ANTHROPIC_BASE_URL must use the https:// scheme to prevent transcript \
         exfiltration; got: {trimmed:?}. This is operator-controlled infra config — \
         update the deployment env to use a https:// endpoint."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_base_url_scheme_accepts_https() {
        validate_base_url_scheme("https://api.anthropic.com")
            .expect("https:// must be accepted");
    }

    #[test]
    fn validate_base_url_scheme_accepts_https_with_path() {
        validate_base_url_scheme("https://my-proxy.example.com/v1")
            .expect("https:// with path must be accepted");
    }

    #[test]
    fn validate_base_url_scheme_accepts_http_in_test_cfg() {
        // In cfg(test), http:// is allowed for localhost stubs.
        validate_base_url_scheme("http://127.0.0.1:1")
            .expect("http:// must be accepted in test cfg");
    }

    #[test]
    fn validate_base_url_scheme_rejects_other_schemes() {
        let error = validate_base_url_scheme("ftp://api.anthropic.com")
            .expect_err("ftp:// must be rejected");
        assert!(
            matches!(error, ExtractionError::ProviderUnavailable(_)),
            "got {error:?}"
        );
        assert!(error.to_string().contains("https://"));
    }

    #[test]
    fn validate_base_url_scheme_rejects_empty_like_nonsense() {
        let error = validate_base_url_scheme("api.anthropic.com")
            .expect_err("scheme-less URL must be rejected");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }
}
