use async_trait::async_trait;
use domain::{
    ExtractionError, ExtractionResult, SessionTranscript, TranscriptSkillExtractionService,
};
use serde::{Deserialize, Serialize};

use crate::extraction::{
    http::post_json_with_timeout,
    limits::{validate_extraction_config, validate_transcript_limits},
    prompt_contract::build_ollama_extraction_prompt,
};

// # OllamaExtractor — Local Prompt Ownership
//
// OllamaExtractor builds a natural-language extraction prompt locally and sends it to
// Ollama's `/api/generate` endpoint with `format: "json"` for structured output.
//
// ## Why local prompt ownership?
//
// Unlike Claude (which delegates to an external extraction service), Ollama's
// `/api/generate` endpoint is a raw model interface. It has no extraction-specific
// prompt engineering. Ollama also lacks `tool_choice` support, so all schema and
// quality guidance must be embedded in the prompt text alongside `format: "json"`.
//
// ## Prompt contract
//
// The prompt is built by `prompt_contract::build_ollama_extraction_prompt()`, which
// generates a production-quality extraction prompt covering:
// 1. Extraction target categories (rules, conventions, workflows, error patterns)
// 2. Quality rubric (FME, actionable specificity, correctness, conciseness, blacklist)
// 3. Output format specification matching `ExtractedSkillCandidate` schema
// 4. Anti-pattern warnings (generic skills, context-dependent skills, non-actionable)
// 5. Confidence scoring guidance
// 6. A concrete example candidate
//
// See `prompt_contract.rs` for the semantic contract shared with the Claude endpoint.
//
// ## T14 Enhancement
//
// Original prompt was a single line: "Extract reusable skill candidates as JSON..."
// This was inadequate. The enhanced prompt follows the research-backed quality criteria
// from `docs/research/2026-05-26-llm-extraction-quality-map-reduce.md` and the SkillLens
// paper (arXiv:2605.23899).

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

        let mut transcript_lines = String::new();
        for entry in &transcript.entries {
            transcript_lines.push_str(&format!("{}: {}\n", entry.speaker, entry.content));
        }

        let prompt = build_ollama_extraction_prompt(&transcript_lines);

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
