use async_trait::async_trait;
use domain::{
    ExtractionError, ExtractionResult, SessionTranscript, TranscriptSkillExtractionService,
};
use serde::{Deserialize, Serialize};

use crate::extraction::{
    http::post_json_with_timeout,
    limits::{validate_extraction_config, validate_transcript_limits},
};

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

        validate_extraction_config(
            config.timeout_ms,
            config.max_entries,
            config.max_entry_chars,
            config.max_total_chars,
        )?;

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

        let raw: OllamaExtractionResponse = post_json_with_timeout(
            &self.client,
            &self.config.endpoint,
            &request,
            self.config.timeout_ms,
            "ollama",
        )
        .await?;
        let parsed: StructuredExtraction = serde_json::from_str(&raw.response)
            .map_err(|error| ExtractionError::Unexpected(error.to_string()))?;

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
