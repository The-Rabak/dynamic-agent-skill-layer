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
pub struct OllamaExtractionConfig {
    pub endpoint: String,
    pub model: String,
    pub timeout_ms: u64,
    pub max_entries: usize,
    pub max_entry_chars: usize,
    pub max_total_chars: usize,
}

impl Default for OllamaExtractionConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434/api/generate".to_owned(),
            model: "llama3.1".to_owned(),
            timeout_ms: 1_500,
            max_entries: 2_000,
            max_entry_chars: 8_192,
            max_total_chars: 1_000_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaExtractor {
    client: reqwest::Client,
    config: OllamaExtractionConfig,
}

impl OllamaExtractor {
    pub fn new(
        client: reqwest::Client,
        config: OllamaExtractionConfig,
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
struct OllamaExtractionRequest {
    model: String,
    stream: bool,
    format: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct OllamaExtractionResponse {
    response: String,
}

#[derive(Debug, Deserialize)]
struct StructuredExtraction {
    #[serde(default)]
    candidates: Vec<domain::ExtractedSkillCandidate>,
}

#[async_trait]
impl TranscriptSkillExtractionService for OllamaExtractor {
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

        let mut prompt = String::from(
            "Extract reusable skill candidates as JSON with a top-level `candidates` array.\n",
        );
        for entry in &transcript.entries {
            prompt.push_str(&format!("{}: {}\n", entry.speaker, entry.content));
        }

        let request = OllamaExtractionRequest {
            model: self.config.model.clone(),
            stream: false,
            format: "json".to_owned(),
            prompt,
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
                    "ollama extraction endpoint returned {}",
                    response.status()
                )));
            }

            let raw = response
                .json::<OllamaExtractionResponse>()
                .await
                .map_err(|error| ExtractionError::Unexpected(error.to_string()))?;

            serde_json::from_str::<StructuredExtraction>(&raw.response)
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
            provider: "ollama".to_owned(),
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
            OllamaExtractor::new(reqwest::Client::new(), OllamaExtractionConfig::default())
                .expect("default config should be valid");
        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("session-ollama-empty"),
            entries: vec![],
        };

        let error = extractor
            .extract(&transcript)
            .await
            .expect_err("empty transcript should fail");

        assert!(matches!(error, ExtractionError::InvalidTranscript(_)));
    }

    #[tokio::test]
    async fn extract_rejects_entry_larger_than_limit() {
        let mut config = OllamaExtractionConfig::default();
        config.max_entry_chars = 4;
        let extractor =
            OllamaExtractor::new(reqwest::Client::new(), config).expect("config should be valid");
        let transcript = SessionTranscript {
            session_id: DomainId::new_unchecked("session-ollama-limit"),
            entries: vec![TranscriptEntry {
                speaker: "user".to_owned(),
                content: "exceeds".to_owned(),
            }],
        };

        let error = extractor
            .extract(&transcript)
            .await
            .expect_err("entry size overflow should fail");

        assert!(matches!(error, ExtractionError::InvalidTranscript(_)));
    }
}
