//! Provider-agnostic structured-text LLM transport for the orchestration seams.
//!
//! The session-extractor orchestration pipeline (skeleton labeling, synthesis,
//! preamble normalization, merge-equivalence) issues many small "prompt → JSON
//! text" calls. Historically each seam called [`crate::ollama_generate_text`]
//! directly, hard-coding Ollama into the real app-logic path regardless of
//! `EXTRACT_SESSION_PROVIDER`. That meant selecting `claude-code` only moved the
//! *map* step to Claude while every seam still hit Ollama.
//!
//! [`StructuredTextLlm`] is the seam transport seam: one trait, two production
//! impls — [`OllamaTextLlm`] (local `/api/generate`) and [`ClaudeCodeTextLlm`]
//! (the host `claude` CLI subscription). `SessionExtractor::from_environment`
//! builds ONE of these per provider and powers all four seams from it, so the
//! whole LLM workload — *everything except embeddings* — runs on the selected
//! provider end to end.
//!
//! ## Fail discipline
//! Both impls fail loud (`ExtractionError::ProviderUnavailable` / `Timeout` /
//! `Unexpected`) when the backend is unreachable or returns an unparseable body.
//! There is NO silent stub result — an unavailable transport is a loud failure
//! (repository mandate).

use async_trait::async_trait;
use domain::ExtractionError;

use crate::ClaudeCodeExtractionConfig;
use crate::extraction::claude_code::claude_code_generate_text;
use crate::extraction::http::{
    OllamaGenerateTextOptions, OllamaGenerateTextRequest, ollama_generate_text,
};

/// A provider-agnostic "prompt → JSON text" transport.
///
/// Implementations send a single deterministic (temperature 0) request and return
/// the model's reply as a JSON string for the caller to parse into its own domain
/// type. This is the seam-level analogue of the extraction provider trait: it
/// carries no extraction semantics, only "structured text in, JSON text out".
#[async_trait]
pub trait StructuredTextLlm: Send + Sync + std::fmt::Debug {
    /// Sends `prompt` and returns the model's raw JSON-text response.
    ///
    /// # Errors
    /// - `ExtractionError::ProviderUnavailable` — backend unreachable / non-200 / non-success.
    /// - `ExtractionError::Timeout` — the call exceeded the provider's timeout.
    /// - `ExtractionError::Unexpected` — unparseable response body.
    async fn generate_json(&self, prompt: String) -> Result<String, ExtractionError>;

    /// Canonical provider label (`"ollama"` / `"claude-code"`) for logging.
    fn provider_label(&self) -> &'static str;

    /// The model identifier in use, for observability.
    fn model(&self) -> &str;
}

// ─── Ollama-backed transport ──────────────────────────────────────────────────

/// [`StructuredTextLlm`] backed by Ollama's `/api/generate` endpoint.
///
/// Issues `format:"json"`, `think:false`, `temperature:0` requests — the same
/// shape the seams used inline before this abstraction, so behaviour for the
/// Ollama path is unchanged.
#[derive(Debug, Clone)]
pub struct OllamaTextLlm {
    client: reqwest::Client,
    endpoint: String,
    model: String,
}

impl OllamaTextLlm {
    /// Constructs the Ollama transport.
    ///
    /// `endpoint` must be the full `/api/generate` URL. Fails loud at construction
    /// when `endpoint` or `model` is blank.
    pub fn new(endpoint: String, model: String) -> Result<Self, ExtractionError> {
        if endpoint.trim().is_empty() || model.trim().is_empty() {
            return Err(ExtractionError::ProviderUnavailable(
                "OllamaTextLlm: endpoint and model must not be blank".to_owned(),
            ));
        }
        Ok(Self {
            client: reqwest::Client::new(),
            endpoint,
            model,
        })
    }
}

#[async_trait]
impl StructuredTextLlm for OllamaTextLlm {
    async fn generate_json(&self, prompt: String) -> Result<String, ExtractionError> {
        let request = OllamaGenerateTextRequest {
            model: self.model.clone(),
            stream: false,
            format: "json".to_owned(),
            prompt,
            // #176: never let a thinking model leak reasoning into the JSON keys.
            think: false,
            options: Some(OllamaGenerateTextOptions { temperature: 0.0 }),
        };
        ollama_generate_text(&self.client, &self.endpoint, &request).await
    }

    fn provider_label(&self) -> &'static str {
        "ollama"
    }

    fn model(&self) -> &str {
        &self.model
    }
}

// ─── Claude Code CLI-backed transport ─────────────────────────────────────────

/// [`StructuredTextLlm`] backed by the host `claude` CLI subscription (no API key).
///
/// Host-only — the `claude` binary must be installed and authenticated where this
/// runs (true on a developer host, NOT in the stock compose container). Each call
/// spawns one `claude -p --output-format json` subprocess via the shared
/// [`claude_code_generate_text`] transport.
#[derive(Debug, Clone)]
pub struct ClaudeCodeTextLlm {
    config: ClaudeCodeExtractionConfig,
}

impl ClaudeCodeTextLlm {
    /// Constructs the claude-code transport from a [`ClaudeCodeExtractionConfig`].
    ///
    /// The transcript-limit fields of the config are irrelevant to seam calls;
    /// only `cli_path`, `model`, and `timeout_ms` are used. Fails loud when
    /// `cli_path` or `model` is blank.
    pub fn new(config: ClaudeCodeExtractionConfig) -> Result<Self, ExtractionError> {
        if config.cli_path.trim().is_empty() || config.model.trim().is_empty() {
            return Err(ExtractionError::ProviderUnavailable(
                "ClaudeCodeTextLlm: cli_path and model must not be blank".to_owned(),
            ));
        }
        Ok(Self { config })
    }
}

#[async_trait]
impl StructuredTextLlm for ClaudeCodeTextLlm {
    async fn generate_json(&self, prompt: String) -> Result<String, ExtractionError> {
        claude_code_generate_text(&self.config, &prompt).await
    }

    fn provider_label(&self) -> &'static str {
        "claude-code"
    }

    fn model(&self) -> &str {
        &self.config.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_text_llm_rejects_blank_endpoint() {
        let error = OllamaTextLlm::new(String::new(), "gemma4:12b".to_owned())
            .expect_err("blank endpoint must fail");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[test]
    fn ollama_text_llm_rejects_blank_model() {
        let error = OllamaTextLlm::new("http://x/api/generate".to_owned(), "  ".to_owned())
            .expect_err("blank model must fail");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[test]
    fn ollama_text_llm_reports_provider_and_model() {
        let llm = OllamaTextLlm::new("http://x/api/generate".to_owned(), "gemma4:12b".to_owned())
            .expect("valid");
        assert_eq!(llm.provider_label(), "ollama");
        assert_eq!(llm.model(), "gemma4:12b");
    }

    #[test]
    fn claude_code_text_llm_rejects_blank_model() {
        let config = ClaudeCodeExtractionConfig {
            model: String::new(),
            ..ClaudeCodeExtractionConfig::default()
        };
        let error = ClaudeCodeTextLlm::new(config).expect_err("blank model must fail");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[test]
    fn claude_code_text_llm_reports_provider_and_default_model() {
        let llm =
            ClaudeCodeTextLlm::new(ClaudeCodeExtractionConfig::default()).expect("valid default");
        assert_eq!(llm.provider_label(), "claude-code");
        assert_eq!(llm.model(), "claude-sonnet-4-6");
    }
}
