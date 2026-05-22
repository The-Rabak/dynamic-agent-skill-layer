use std::time::Duration;

use async_trait::async_trait;
use domain::{
    ExtractionError, ExtractionResult, SessionTranscript, TranscriptSkillExtractionService,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use crate::extraction::limits::validate_transcript_limits;

#[derive(Debug, Clone)]
pub struct ClaudeExtractionConfig {
    pub endpoint: String,
    pub model: String,
    pub timeout_ms: u64,
    pub max_entries: usize,
    pub max_entry_chars: usize,
    pub max_total_chars: usize,
}

impl Default for ClaudeExtractionConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8080/extract".to_owned(),
            model: "claude-sonnet".to_owned(),
            timeout_ms: 1_500,
            max_entries: 2_000,
            max_entry_chars: 8_192,
            max_total_chars: 1_000_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeExtractor {
    client: reqwest::Client,
    config: ClaudeExtractionConfig,
}

impl ClaudeExtractor {
    pub fn new(
        client: reqwest::Client,
        config: ClaudeExtractionConfig,
    ) -> Result<Self, ExtractionError> {
        if config.endpoint.trim().is_empty() || config.model.trim().is_empty() {
            return Err(ExtractionError::InvalidTranscript(
                "extraction provider configuration must not be blank".to_owned(),
            ));
        }

        if config.timeout_ms == 0 {
            return Err(ExtractionError::InvalidTranscript(
                "extraction timeout must be greater than zero".to_owned(),
            ));
        }

        if config.max_entries == 0 || config.max_entry_chars == 0 || config.max_total_chars == 0 {
            return Err(ExtractionError::InvalidTranscript(
                "transcript limits must be greater than zero".to_owned(),
            ));
        }

        Ok(Self { client, config })
    }
}

#[derive(Debug, Serialize)]
struct ExtractionRequest {
    model: String,
    session_id: String,
    transcript: Vec<TranscriptEntryPayload>,
}

#[derive(Debug, Serialize)]
struct TranscriptEntryPayload {
    speaker: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ExtractionResponse {
    #[serde(default)]
    candidates: Vec<domain::ExtractedSkillCandidate>,
    #[serde(default)]
    provider: Option<String>,
}

#[async_trait]
impl TranscriptSkillExtractionService for ClaudeExtractor {
    async fn extract(
        &self,
        transcript: &SessionTranscript,
    ) -> Result<ExtractionResult, ExtractionError> {
        if transcript.entries.is_empty() {
            return Err(ExtractionError::InvalidTranscript(
                "transcript must include at least one entry".to_owned(),
            ));
        }
        validate_transcript_limits(
            transcript,
            self.config.max_entries,
            self.config.max_entry_chars,
            self.config.max_total_chars,
        )?;

        let request = ExtractionRequest {
            model: self.config.model.clone(),
            session_id: transcript.session_id.as_str().to_owned(),
            transcript: transcript
                .entries
                .iter()
                .map(|entry| TranscriptEntryPayload {
                    speaker: entry.speaker.clone(),
                    content: entry.content.clone(),
                })
                .collect(),
        };

        let parsed = timeout(Duration::from_millis(self.config.timeout_ms), async {
            let response = self
                .client
                .post(&self.config.endpoint)
                .json(&request)
                .send()
                .await
                .map_err(|error| ExtractionError::ProviderUnavailable(error.to_string()))?;

            if response.status() != StatusCode::OK {
                return Err(ExtractionError::ProviderUnavailable(format!(
                    "claude extraction endpoint returned {}",
                    response.status()
                )));
            }

            response
                .json::<ExtractionResponse>()
                .await
                .map_err(|error| ExtractionError::Unexpected(error.to_string()))
        })
        .await
        .map_err(|_| ExtractionError::Timeout {
            timeout_ms: self.config.timeout_ms,
        })
        .and_then(|result| result)?;

        Ok(ExtractionResult {
            source_session_id: transcript.session_id.clone(),
            candidates: parsed.candidates,
            provider: parsed.provider.unwrap_or_else(|| "claude".to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{DomainId, TranscriptEntry};

    #[tokio::test]
    async fn extract_rejects_empty_transcript() {
        let extractor =
            ClaudeExtractor::new(reqwest::Client::new(), ClaudeExtractionConfig::default())
                .expect("default config should be valid");
        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("session-001"),
            entries: vec![],
        };

        let error = extractor
            .extract(&transcript)
            .await
            .expect_err("empty transcript should fail");

        assert!(matches!(error, ExtractionError::InvalidTranscript(_)));
    }

    #[tokio::test]
    async fn extract_uses_provider_unavailable_for_connection_errors() {
        let mut config = ClaudeExtractionConfig::default();
        config.endpoint = "http://127.0.0.1:1/extract".to_owned();

        let extractor =
            ClaudeExtractor::new(reqwest::Client::new(), config).expect("config should be valid");

        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("session-002"),
            entries: vec![TranscriptEntry {
                speaker: "user".to_owned(),
                content: "extract this".to_owned(),
            }],
        };

        let error = extractor
            .extract(&transcript)
            .await
            .expect_err("unreachable provider should fail");

        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[tokio::test]
    async fn extract_rejects_transcript_exceeding_limit() {
        let mut config = ClaudeExtractionConfig::default();
        config.max_entries = 1;
        let extractor =
            ClaudeExtractor::new(reqwest::Client::new(), config).expect("config should be valid");

        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("session-003"),
            entries: vec![
                TranscriptEntry {
                    speaker: "user".to_owned(),
                    content: "first".to_owned(),
                },
                TranscriptEntry {
                    speaker: "assistant".to_owned(),
                    content: "second".to_owned(),
                },
            ],
        };

        let error = extractor
            .extract(&transcript)
            .await
            .expect_err("limit overflow should fail");

        assert!(matches!(error, ExtractionError::InvalidTranscript(_)));
    }
}
