use std::time::Duration;

use async_trait::async_trait;
use domain::ExtractionError;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use crate::extraction::{
    http::post_json_with_timeout,
    prompt_contract::DEFAULT_CLAUDE_MODEL,
};

/// Default Anthropic API base URL for the generality verifier Claude path.
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
/// Anthropic API version header required by the Messages API.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Maximum output tokens for a single generality decision — the response is small.
const MAX_GENERALITY_OUTPUT_TOKENS: u32 = 256;
/// Forced tool name for structured generality emission.
const GENERALITY_TOOL_NAME: &str = "emit_generality_decision";

// ─── Narrow async LLM generality trait ───────────────────────────────────────

/// Decision returned by LLM skill-generality providers.
///
/// Carries both the binary decision and a human-readable rationale so callers
/// can log why a skill was (or was not) proposed for global promotion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneralityDecision {
    /// `true` when the LLM considers the skill description tool- or language-general
    /// (not dependent on a specific codebase) and a global promotion proposal is warranted.
    pub general: bool,
    /// Free-text explanation from the LLM, used for audit logs and proposal frontmatter.
    pub rationale: String,
}

/// Narrow async trait for LLM-backed skill generality verification.
///
/// Implementations receive a skill text block and return a structured `GeneralityDecision`.
/// Unavailable providers must surface `ExtractionError::ProviderUnavailable` or
/// `ExtractionError::Timeout` — never a silent `general=false`.
///
/// This seam mirrors `LlmEquivalenceVerifier` but is purpose-built for the promotion
/// intrinsic path: the question is "is this lesson general?" not "are these two skills
/// the same?".
#[async_trait]
pub trait SkillGeneralityVerifier: Send + Sync {
    /// Asks the LLM whether `skill_text` describes a tool/language-general lesson
    /// (promotes to global) or is specific to one codebase (stays project).
    ///
    /// # Errors
    ///
    /// - `ExtractionError::ProviderUnavailable` — the provider endpoint could not
    ///   be reached, or returned a non-200 status.
    /// - `ExtractionError::Timeout` — the request exceeded `timeout_ms`.
    /// - `ExtractionError::Unexpected` — unexpected response shape.
    async fn decide_generality(
        &self,
        skill_text: &str,
    ) -> Result<GeneralityDecision, ExtractionError>;
}

// ─── Ollama /api/generate adapter ────────────────────────────────────────────

/// Configuration for the Ollama-backed skill generality verifier.
#[derive(Debug, Clone)]
pub struct OllamaGeneralityVerifierConfig {
    /// Full URL to Ollama's `/api/generate` endpoint (e.g. `http://ollama:11434/api/generate`).
    pub endpoint: String,
    /// Ollama model to use for generality decisions.
    pub model: String,
    /// Per-request timeout in milliseconds.
    pub timeout_ms: u64,
}

impl Default for OllamaGeneralityVerifierConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434/api/generate".to_owned(),
            // Same default as the merge verifier path; override via GENERALITY_VERIFIER_MODEL.
            model: "gemma4:12b".to_owned(),
            // Generality verification is a short prompt; 60s is generous on CPU inference.
            timeout_ms: 60_000,
        }
    }
}

/// Ollama-backed LLM generality verifier using `/api/generate`.
///
/// Sends a structured JSON prompt to Ollama and parses `{general, rationale}`.
/// Temperature is fixed to `0` for deterministic (greedy) decisions.
/// Fails loudly (`ExtractionError::ProviderUnavailable`) on connection errors or
/// non-200 responses — never silently returns `general=false`.
#[derive(Debug, Clone)]
pub struct OllamaGeneralityVerifier {
    client: reqwest::Client,
    config: OllamaGeneralityVerifierConfig,
}

impl OllamaGeneralityVerifier {
    /// Constructs the Ollama generality verifier.
    ///
    /// Fails at construct time when the endpoint or model is blank.
    pub fn new(
        client: reqwest::Client,
        config: OllamaGeneralityVerifierConfig,
    ) -> Result<Self, ExtractionError> {
        if config.endpoint.trim().is_empty() || config.model.trim().is_empty() {
            return Err(ExtractionError::ProviderUnavailable(
                "GENERALITY_VERIFIER_MODEL and the Ollama endpoint must not be blank".to_owned(),
            ));
        }
        Ok(Self { client, config })
    }

    /// Constructs the Ollama generality verifier with a new shared HTTP client.
    ///
    /// Preferred constructor for callers that do not need to inject a custom client.
    pub fn from_config(config: OllamaGeneralityVerifierConfig) -> Result<Self, ExtractionError> {
        Self::new(reqwest::Client::new(), config)
    }
}

#[derive(Debug, Serialize)]
struct OllamaGenerateOptions {
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct OllamaGenerateRequest {
    model: String,
    stream: bool,
    format: String,
    prompt: String,
    options: OllamaGenerateOptions,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

/// Intermediate JSON shape returned inside `OllamaGenerateResponse.response`.
#[derive(Debug, Deserialize)]
struct OllamaGeneralityPayload {
    general: bool,
    rationale: String,
}

#[async_trait]
impl SkillGeneralityVerifier for OllamaGeneralityVerifier {
    async fn decide_generality(
        &self,
        skill_text: &str,
    ) -> Result<GeneralityDecision, ExtractionError> {
        let prompt = build_generality_prompt(skill_text);
        let request = OllamaGenerateRequest {
            model: self.config.model.clone(),
            stream: false,
            format: "json".to_owned(),
            prompt,
            options: OllamaGenerateOptions { temperature: 0.0 },
        };

        let raw: OllamaGenerateResponse = post_json_with_timeout(
            &self.client,
            &self.config.endpoint,
            &request,
            self.config.timeout_ms,
            "ollama-generality-verifier",
        )
        .await?;

        let payload: OllamaGeneralityPayload =
            serde_json::from_str(&raw.response).map_err(|error| {
                ExtractionError::Unexpected(format!(
                    "ollama generality-verifier response was not valid JSON \
                     {{general, rationale}}: {error}"
                ))
            })?;

        Ok(GeneralityDecision {
            general: payload.general,
            rationale: payload.rationale,
        })
    }
}

// ─── Claude Messages API adapter ─────────────────────────────────────────────

/// Configuration for the Claude-backed skill generality verifier.
#[derive(Debug, Clone)]
pub struct ClaudeGeneralityVerifierConfig {
    /// Anthropic API base URL (no trailing `/v1/messages`).
    pub base_url: String,
    /// Anthropic API key. Required — fails loudly at construct time when absent.
    pub api_key: String,
    /// Claude model to use for generality decisions.
    pub model: String,
    /// Per-request timeout in milliseconds.
    pub timeout_ms: u64,
}

impl Default for ClaudeGeneralityVerifierConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_ANTHROPIC_BASE_URL.to_owned(),
            api_key: String::new(),
            model: DEFAULT_CLAUDE_MODEL.to_owned(),
            // Cloud inference is fast; 30s is generous.
            timeout_ms: 30_000,
        }
    }
}

/// Claude-backed LLM generality verifier using the Anthropic Messages API
/// with a forced `tool_use` for structured `{general, rationale}` output.
///
/// Requires `ANTHROPIC_API_KEY` — missing key fails loudly at construct time
/// (Constitution Principle 1). Unavailable endpoint surfaces as
/// `ExtractionError::ProviderUnavailable`, never a silent `general=false`.
#[derive(Debug, Clone)]
pub struct ClaudeGeneralityVerifier {
    client: reqwest::Client,
    config: ClaudeGeneralityVerifierConfig,
    messages_endpoint: String,
}

impl ClaudeGeneralityVerifier {
    /// Constructs the Claude generality verifier.
    ///
    /// Fails loudly at construct time when:
    /// - `api_key` is blank (Constitution Principle 1)
    /// - `base_url` or `model` is blank
    pub fn new(
        client: reqwest::Client,
        config: ClaudeGeneralityVerifierConfig,
    ) -> Result<Self, ExtractionError> {
        if config.base_url.trim().is_empty() || config.model.trim().is_empty() {
            return Err(ExtractionError::ProviderUnavailable(
                "Claude generality verifier configuration must not be blank".to_owned(),
            ));
        }
        if config.api_key.trim().is_empty() {
            return Err(ExtractionError::ProviderUnavailable(
                "ANTHROPIC_API_KEY must be set to use the Claude generality verifier".to_owned(),
            ));
        }
        let messages_endpoint =
            format!("{}/v1/messages", config.base_url.trim_end_matches('/'));
        Ok(Self {
            client,
            config,
            messages_endpoint,
        })
    }

    /// Constructs the Claude generality verifier with a new shared HTTP client.
    ///
    /// Preferred constructor for callers that do not need to inject a custom client.
    pub fn from_config(config: ClaudeGeneralityVerifierConfig) -> Result<Self, ExtractionError> {
        Self::new(reqwest::Client::new(), config)
    }
}

/// Minimal Messages API request for the generality tool call.
#[derive(Debug, Serialize)]
struct GeneralityMessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<GeneralityMessageBlock>,
    tools: Vec<GeneralityToolDefinition<'a>>,
    tool_choice: GeneralityToolChoice<'a>,
}

#[derive(Debug, Serialize)]
struct GeneralityMessageBlock {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct GeneralityToolDefinition<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct GeneralityToolChoice<'a> {
    #[serde(rename = "type")]
    choice_type: &'a str,
    name: &'a str,
}

#[derive(Debug, Deserialize)]
struct GeneralityMessagesResponse {
    #[serde(default)]
    content: Vec<GeneralityContentBlock>,
}

#[derive(Debug, Deserialize)]
struct GeneralityContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<GeneralityToolInput>,
}

#[derive(Debug, Deserialize)]
struct GeneralityToolInput {
    general: bool,
    rationale: String,
}

/// Returns the forced-tool input schema for the generality decision.
fn generality_tool_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "general": {
                "type": "boolean",
                "description": "true if the skill is tool/language-general (not codebase-specific)"
            },
            "rationale": {
                "type": "string",
                "description": "One-sentence explanation for the decision"
            }
        },
        "required": ["general", "rationale"]
    })
}

#[async_trait]
impl SkillGeneralityVerifier for ClaudeGeneralityVerifier {
    async fn decide_generality(
        &self,
        skill_text: &str,
    ) -> Result<GeneralityDecision, ExtractionError> {
        crate::extraction::http::acquire_claude_rate_limit().await?;

        let user_content = build_generality_prompt(skill_text);
        let request = GeneralityMessagesRequest {
            model: &self.config.model,
            max_tokens: MAX_GENERALITY_OUTPUT_TOKENS,
            messages: vec![GeneralityMessageBlock {
                role: "user".to_owned(),
                content: user_content,
            }],
            tools: vec![GeneralityToolDefinition {
                name: GENERALITY_TOOL_NAME,
                description: "Emit whether the skill describes a tool/language-general lesson.",
                input_schema: generality_tool_schema(),
            }],
            tool_choice: GeneralityToolChoice {
                choice_type: "tool",
                name: GENERALITY_TOOL_NAME,
            },
        };

        let response: GeneralityMessagesResponse =
            timeout(Duration::from_millis(self.config.timeout_ms), async {
                let http_response = self
                    .client
                    .post(&self.messages_endpoint)
                    .header("x-api-key", &self.config.api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
                    .header("content-type", "application/json")
                    .json(&request)
                    .send()
                    .await
                    .map_err(|error| {
                        ExtractionError::ProviderUnavailable(error.to_string())
                    })?;

                if http_response.status() != StatusCode::OK {
                    return Err(ExtractionError::ProviderUnavailable(format!(
                        "claude generality-verifier endpoint returned {}",
                        http_response.status()
                    )));
                }

                http_response
                    .json::<GeneralityMessagesResponse>()
                    .await
                    .map_err(|error| ExtractionError::Unexpected(error.to_string()))
            })
            .await
            .map_err(|_| ExtractionError::Timeout {
                timeout_ms: self.config.timeout_ms,
            })??;

        let tool_input = response
            .content
            .into_iter()
            .find(|block| {
                block.block_type == "tool_use"
                    && block.name.as_deref() == Some(GENERALITY_TOOL_NAME)
            })
            .and_then(|block| block.input)
            .ok_or_else(|| {
                ExtractionError::Unexpected(
                    "claude generality-verifier response did not contain a forced \
                     emit_generality_decision tool_use block"
                        .to_owned(),
                )
            })?;

        Ok(GeneralityDecision {
            general: tool_input.general,
            rationale: tool_input.rationale,
        })
    }
}

// ─── Shared prompt builder ────────────────────────────────────────────────────

/// Builds the user-facing generality prompt sent to both Ollama and Claude.
///
/// Instructs the model to decide whether the skill text describes a lesson that
/// depends on a specific codebase, or is tool/language-general and therefore
/// suitable for promotion to the global skill scope.
fn build_generality_prompt(skill_text: &str) -> String {
    format!(
        "You are reviewing an agent skill description to decide if it is \
         tool/language-general or specific to one particular codebase.\n\
         \n\
         Skill:\n\
         {skill_text}\n\
         \n\
         Rules:\n\
         - Answer `general: true` when the lesson applies broadly — it is about a \
           language feature, tool behaviour, ecosystem convention, or universal \
           engineering practice that would be useful in ANY project.\n\
         - Answer `general: false` when the lesson depends on this specific codebase — \
           it references project-specific modules, internal identifiers, proprietary \
           conventions, or knowledge that is only meaningful in one repo.\n\
         - Do NOT consider naming style; focus on whether the *substance* of the \
           lesson is universally applicable.\n\
         \n\
         Respond with JSON: {{\"general\": <bool>, \"rationale\": \"<one sentence>\"}}"
    )
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_verifier_rejects_blank_endpoint() {
        let config = OllamaGeneralityVerifierConfig {
            endpoint: String::new(),
            model: "gemma4:12b".to_owned(),
            timeout_ms: 60_000,
        };
        let error = OllamaGeneralityVerifier::new(reqwest::Client::new(), config)
            .expect_err("blank endpoint must be rejected");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[test]
    fn ollama_verifier_rejects_blank_model() {
        let config = OllamaGeneralityVerifierConfig {
            endpoint: "http://127.0.0.1:11434/api/generate".to_owned(),
            model: String::new(),
            timeout_ms: 60_000,
        };
        let error = OllamaGeneralityVerifier::new(reqwest::Client::new(), config)
            .expect_err("blank model must be rejected");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[test]
    fn claude_verifier_rejects_missing_api_key() {
        let config = ClaudeGeneralityVerifierConfig::default(); // api_key is blank
        let error = ClaudeGeneralityVerifier::new(reqwest::Client::new(), config)
            .expect_err("missing API key must fail loudly");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
        assert!(error.to_string().contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn claude_verifier_accepts_valid_config() {
        let config = ClaudeGeneralityVerifierConfig {
            api_key: "test-key".to_owned(),
            ..ClaudeGeneralityVerifierConfig::default()
        };
        let verifier = ClaudeGeneralityVerifier::new(reqwest::Client::new(), config)
            .expect("valid config must succeed");
        assert_eq!(
            verifier.messages_endpoint,
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn generality_prompt_contains_skill_text_and_decision_keys() {
        let prompt = build_generality_prompt("declare cargo bin explicitly or binary is named after package");
        assert!(prompt.contains("declare cargo bin explicitly"));
        assert!(prompt.contains("general"));
        assert!(prompt.contains("rationale"));
    }
}
