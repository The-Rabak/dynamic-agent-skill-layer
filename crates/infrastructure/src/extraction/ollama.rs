use async_trait::async_trait;
use domain::{
    ExtractionError, ExtractionResult, SessionTranscript, TranscriptEntry,
    TranscriptSkillExtractionService,
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
// See `extraction/mod.rs` for the full prompt strategy rationale.

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
            // granite4:3b is the V1.5 target local model (small, CPU-friendly).
            model: "granite4:3b".to_owned(),
            // 120s inner timeout for CPU inference. NOTE: this is an UNMEASURED
            // placeholder — single-job p50/p95 on the target host has not been
            // measured in this environment. The operator must confirm/adjust
            // against the real deployment (override via OLLAMA_EXTRACTION_TIMEOUT_MS).
            // The worker-pool (outer) timeout must stay >= 1.5x this value.
            timeout_ms: 120_000,
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

/// Known jailbreak prefixes that indicate prompt-injection attempts in transcript content.
const JAILBREAK_PREFIXES: &[&str] = &[
    "Ignore previous instructions",
    "You are now",
    "Override",
    "SYSTEM PROMPT",
    "New instructions",
    "Disregard",
];

/// Known speaker names used to impersonate system or assistant roles.
const SUSPICIOUS_SPEAKERS: &[&str] = &[
    "system",
    "System",
    "assistant",
    "Assistant",
    "SYSTEM",
    "ASSISTANT",
];

/// Sanitizes a single transcript entry before it enters the prompt.
///
/// Returns `None` if the entry should be dropped entirely (suspicious speaker or
/// jailbreak prefix). Otherwise returns the sanitized content string with control
/// characters stripped.
fn sanitize_transcript_entry(entry: &TranscriptEntry) -> Option<String> {
    // Reject entries where the speaker impersonates system/assistant roles
    if SUSPICIOUS_SPEAKERS
        .iter()
        .any(|s| entry.speaker.contains(s))
    {
        return None;
    }

    // Strip control characters (keep only printable ASCII + newline)
    let cleaned: String = entry
        .content
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ' || *c == '\n')
        .collect();

    // Reject entries whose content starts with a known jailbreak prefix
    if JAILBREAK_PREFIXES
        .iter()
        .any(|prefix| cleaned.starts_with(*prefix))
    {
        return None;
    }

    Some(cleaned)
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
            if let Some(sanitized) = sanitize_transcript_entry(entry) {
                transcript_lines.push_str(&format!("{}: {}\n", entry.speaker, sanitized));
            }
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

    #[test]
    fn default_config_targets_granite_with_cpu_inference_timeout() {
        let config = OllamaExtractionConfig::default();
        assert_eq!(
            config.model, "granite4:3b",
            "default Ollama model must be granite4:3b"
        );
        assert!(
            config.timeout_ms >= 60_000,
            "inner timeout must be realistic for CPU inference (>=60s), got {}ms",
            config.timeout_ms
        );
    }

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
