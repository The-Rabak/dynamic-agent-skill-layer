use std::time::Duration;

use async_trait::async_trait;
use domain::ExtractionError;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use crate::extraction::{http::post_json, prompt_contract::DEFAULT_CLAUDE_MODEL};

/// Default Anthropic API base URL for the merge verifier Claude path.
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
/// Anthropic API version header required by the Messages API.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Maximum output tokens for a single equivalence decision — the response is small.
const MAX_EQUIVALENCE_OUTPUT_TOKENS: u32 = 256;
/// Forced tool name for structured equivalence emission.
const EQUIVALENCE_TOOL_NAME: &str = "emit_equivalence_decision";

// ─── Narrow async LLM equivalence trait ──────────────────────────────────────

/// Decision returned by LLM semantic equivalence providers.
///
/// Carries both the binary decision and a human-readable rationale so callers
/// can log why a pair was (or was not) proposed for merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquivalenceDecision {
    /// `true` when the LLM considers the two skill descriptions semantically
    /// equivalent and a merge proposal is warranted.
    pub equivalent: bool,
    /// Free-text explanation from the LLM, used for audit logs.
    pub rationale: String,
}

/// Narrow async trait consumed by [`LlmMergeSemanticVerifier`] in the maintenance
/// crate.
///
/// Implementations receive two skill text blocks and return a structured
/// `EquivalenceDecision`. Unavailable providers must surface `ExtractionError::ProviderUnavailable`
/// or `ExtractionError::Timeout` — never a silent `equivalent=false`.
#[async_trait]
pub trait LlmEquivalenceVerifier: Send + Sync {
    /// Asks the LLM whether `left_text` and `right_text` describe the same
    /// reusable skill.
    ///
    /// # Errors
    ///
    /// - `ExtractionError::ProviderUnavailable` — the provider endpoint could not
    ///   be reached, or returned a non-200 status.
    /// - `ExtractionError::Timeout` — the request exceeded `timeout_ms`.
    /// - `ExtractionError::Unexpected` — unexpected response shape.
    async fn decide_equivalence(
        &self,
        left_text: &str,
        right_text: &str,
    ) -> Result<EquivalenceDecision, ExtractionError>;
}

// ─── Ollama /api/generate adapter ────────────────────────────────────────────

/// Configuration for the Ollama-backed merge equivalence verifier.
#[derive(Debug, Clone)]
pub struct OllamaMergeVerifierConfig {
    /// Full URL to Ollama's `/api/generate` endpoint (e.g. `http://ollama:11434/api/generate`).
    pub endpoint: String,
    /// Ollama model to use for equivalence decisions.
    pub model: String,
}

impl Default for OllamaMergeVerifierConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434/api/generate".to_owned(),
            // Same default as the extraction path; override via MERGE_VERIFIER_MODEL.
            model: "gemma4:12b".to_owned(),
        }
    }
}

/// Ollama-backed LLM equivalence verifier using `/api/generate`.
///
/// Sends a structured JSON prompt to Ollama and parses `{equivalent, rationale}`.
/// Temperature is fixed to `0` for deterministic (greedy) decisions.
/// Fails loudly (`ExtractionError::ProviderUnavailable`) on connection errors or
/// non-200 responses — never silently returns `equivalent=false`.
#[derive(Debug, Clone)]
pub struct OllamaMergeVerifier {
    client: reqwest::Client,
    config: OllamaMergeVerifierConfig,
}

impl OllamaMergeVerifier {
    /// Constructs the Ollama merge verifier.
    ///
    /// Fails at construct time when the endpoint or model is blank.
    pub fn new(
        client: reqwest::Client,
        config: OllamaMergeVerifierConfig,
    ) -> Result<Self, ExtractionError> {
        if config.endpoint.trim().is_empty() || config.model.trim().is_empty() {
            return Err(ExtractionError::ProviderUnavailable(
                "MERGE_VERIFIER_MODEL and the Ollama endpoint must not be blank".to_owned(),
            ));
        }
        Ok(Self { client, config })
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
    /// Disables thinking-mode leak into the JSON output (#176). gemma4:12b otherwise
    /// emits chain-of-thought as JSON keys instead of the contracted equivalence shape.
    think: bool,
    options: OllamaGenerateOptions,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

/// Intermediate JSON shape returned inside `OllamaGenerateResponse.response`.
#[derive(Debug, Deserialize)]
struct OllamaEquivalencePayload {
    equivalent: bool,
    rationale: String,
}

#[async_trait]
impl LlmEquivalenceVerifier for OllamaMergeVerifier {
    async fn decide_equivalence(
        &self,
        left_text: &str,
        right_text: &str,
    ) -> Result<EquivalenceDecision, ExtractionError> {
        let prompt = build_equivalence_prompt(left_text, right_text);
        let request = OllamaGenerateRequest {
            model: self.config.model.clone(),
            stream: false,
            format: "json".to_owned(),
            prompt,
            think: false,
            options: OllamaGenerateOptions { temperature: 0.0 },
        };

        let raw: OllamaGenerateResponse = post_json(
            &self.client,
            &self.config.endpoint,
            &request,
            "ollama-merge-verifier",
        )
        .await?;

        let payload: OllamaEquivalencePayload =
            serde_json::from_str(&raw.response).map_err(|error| {
                ExtractionError::Unexpected(format!(
                    "ollama merge-verifier response was not valid JSON \
                     {{equivalent, rationale}}: {error}"
                ))
            })?;

        Ok(EquivalenceDecision {
            equivalent: payload.equivalent,
            rationale: payload.rationale,
        })
    }
}

// ─── Claude Messages API adapter ─────────────────────────────────────────────

/// Configuration for the Claude-backed merge equivalence verifier.
#[derive(Debug, Clone)]
pub struct ClaudeMergeVerifierConfig {
    /// Anthropic API base URL (no trailing `/v1/messages`).
    pub base_url: String,
    /// Anthropic API key. Required — fails loudly at construct time when absent.
    pub api_key: String,
    /// Claude model to use for equivalence decisions.
    pub model: String,
    /// Per-request timeout in milliseconds.
    pub timeout_ms: u64,
}

impl Default for ClaudeMergeVerifierConfig {
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

/// Claude-backed LLM equivalence verifier using the Anthropic Messages API
/// with a forced `tool_use` for structured `{equivalent, rationale}` output.
///
/// Requires `ANTHROPIC_API_KEY` — missing key fails loudly at construct time
/// (Constitution Principle 1). Unavailable endpoint surfaces as
/// `ExtractionError::ProviderUnavailable`, never a silent `equivalent=false`.
#[derive(Debug, Clone)]
pub struct ClaudeMergeVerifier {
    client: reqwest::Client,
    config: ClaudeMergeVerifierConfig,
    messages_endpoint: String,
}

impl ClaudeMergeVerifier {
    /// Constructs the Claude merge verifier.
    ///
    /// Fails loudly at construct time when:
    /// - `api_key` is blank (Constitution Principle 1)
    /// - `base_url` or `model` is blank
    pub fn new(
        client: reqwest::Client,
        config: ClaudeMergeVerifierConfig,
    ) -> Result<Self, ExtractionError> {
        if config.base_url.trim().is_empty() || config.model.trim().is_empty() {
            return Err(ExtractionError::ProviderUnavailable(
                "Claude merge verifier configuration must not be blank".to_owned(),
            ));
        }
        if config.api_key.trim().is_empty() {
            return Err(ExtractionError::ProviderUnavailable(
                "ANTHROPIC_API_KEY must be set to use the Claude merge verifier".to_owned(),
            ));
        }
        let messages_endpoint = format!("{}/v1/messages", config.base_url.trim_end_matches('/'));
        Ok(Self {
            client,
            config,
            messages_endpoint,
        })
    }
}

/// Minimal Messages API request for the equivalence tool call.
#[derive(Debug, Serialize)]
struct EquivalenceMessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<EquivalenceMessageBlock>,
    tools: Vec<EquivalenceToolDefinition<'a>>,
    tool_choice: EquivalenceToolChoice<'a>,
}

#[derive(Debug, Serialize)]
struct EquivalenceMessageBlock {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct EquivalenceToolDefinition<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct EquivalenceToolChoice<'a> {
    #[serde(rename = "type")]
    choice_type: &'a str,
    name: &'a str,
}

#[derive(Debug, Deserialize)]
struct EquivalenceMessagesResponse {
    #[serde(default)]
    content: Vec<EquivalenceContentBlock>,
}

#[derive(Debug, Deserialize)]
struct EquivalenceContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<EquivalenceToolInput>,
}

#[derive(Debug, Deserialize)]
struct EquivalenceToolInput {
    equivalent: bool,
    rationale: String,
}

/// Returns the forced-tool input schema for the equivalence decision.
fn equivalence_tool_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "equivalent": {
                "type": "boolean",
                "description": "true if the two skills are semantically equivalent"
            },
            "rationale": {
                "type": "string",
                "description": "One-sentence explanation for the decision"
            }
        },
        "required": ["equivalent", "rationale"]
    })
}

#[async_trait]
impl LlmEquivalenceVerifier for ClaudeMergeVerifier {
    async fn decide_equivalence(
        &self,
        left_text: &str,
        right_text: &str,
    ) -> Result<EquivalenceDecision, ExtractionError> {
        crate::extraction::http::acquire_claude_rate_limit().await?;

        let user_content = build_equivalence_prompt(left_text, right_text);
        let request = EquivalenceMessagesRequest {
            model: &self.config.model,
            max_tokens: MAX_EQUIVALENCE_OUTPUT_TOKENS,
            messages: vec![EquivalenceMessageBlock {
                role: "user".to_owned(),
                content: user_content,
            }],
            tools: vec![EquivalenceToolDefinition {
                name: EQUIVALENCE_TOOL_NAME,
                description: "Emit whether the two skills are semantically equivalent.",
                input_schema: equivalence_tool_schema(),
            }],
            tool_choice: EquivalenceToolChoice {
                choice_type: "tool",
                name: EQUIVALENCE_TOOL_NAME,
            },
        };

        let response: EquivalenceMessagesResponse =
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
                    .map_err(|error| ExtractionError::ProviderUnavailable(error.to_string()))?;

                if http_response.status() != StatusCode::OK {
                    return Err(ExtractionError::ProviderUnavailable(format!(
                        "claude merge-verifier endpoint returned {}",
                        http_response.status()
                    )));
                }

                http_response
                    .json::<EquivalenceMessagesResponse>()
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
                    && block.name.as_deref() == Some(EQUIVALENCE_TOOL_NAME)
            })
            .and_then(|block| block.input)
            .ok_or_else(|| {
                ExtractionError::Unexpected(
                    "claude merge-verifier response did not contain a forced \
                     emit_equivalence_decision tool_use block"
                        .to_owned(),
                )
            })?;

        Ok(EquivalenceDecision {
            equivalent: tool_input.equivalent,
            rationale: tool_input.rationale,
        })
    }
}

// ─── Shared prompt builder ────────────────────────────────────────────────────

/// Builds the user-facing equivalence prompt sent to both Ollama and Claude.
///
/// Instructs the model to decide whether the two skill text blocks describe the
/// same reusable skill and to return a structured JSON `{equivalent, rationale}`.
fn build_equivalence_prompt(left_text: &str, right_text: &str) -> String {
    format!(
        "You are reviewing two agent skill descriptions to decide if they are \
         semantically equivalent — that is, if they encode the same reusable \
         knowledge or workflow and should be merged into one skill.\n\
         \n\
         Skill A:\n\
         {left_text}\n\
         \n\
         Skill B:\n\
         {right_text}\n\
         \n\
         Rules:\n\
         - Answer `equivalent: true` only when the skills describe the SAME core \
           procedure or knowledge, even if they use different words.\n\
         - Answer `equivalent: false` when the skills are genuinely distinct — \
           even if they share a topic area or vocabulary.\n\
         - Do NOT consider superficial word overlap; focus on the underlying intent.\n\
         \n\
         Respond with JSON: {{\"equivalent\": <bool>, \"rationale\": \"<one sentence>\"}}"
    )
}

// ─── Convenience constructors ─────────────────────────────────────────────────

impl OllamaMergeVerifier {
    /// Constructs the Ollama merge verifier with a new shared HTTP client.
    ///
    /// Preferred constructor for callers that do not need to inject a custom client.
    pub fn from_config(config: OllamaMergeVerifierConfig) -> Result<Self, ExtractionError> {
        Self::new(reqwest::Client::new(), config)
    }
}

impl ClaudeMergeVerifier {
    /// Constructs the Claude merge verifier with a new shared HTTP client.
    ///
    /// Preferred constructor for callers that do not need to inject a custom client.
    pub fn from_config(config: ClaudeMergeVerifierConfig) -> Result<Self, ExtractionError> {
        Self::new(reqwest::Client::new(), config)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_verifier_rejects_blank_endpoint() {
        let config = OllamaMergeVerifierConfig {
            endpoint: String::new(),
            model: "gemma4:12b".to_owned(),
        };
        let error = OllamaMergeVerifier::new(reqwest::Client::new(), config)
            .expect_err("blank endpoint must be rejected");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[test]
    fn ollama_verifier_rejects_blank_model() {
        let config = OllamaMergeVerifierConfig {
            endpoint: "http://127.0.0.1:11434/api/generate".to_owned(),
            model: String::new(),
        };
        let error = OllamaMergeVerifier::new(reqwest::Client::new(), config)
            .expect_err("blank model must be rejected");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
    }

    #[test]
    fn claude_verifier_rejects_missing_api_key() {
        let config = ClaudeMergeVerifierConfig::default(); // api_key is blank
        let error = ClaudeMergeVerifier::new(reqwest::Client::new(), config)
            .expect_err("missing API key must fail loudly");
        assert!(matches!(error, ExtractionError::ProviderUnavailable(_)));
        assert!(error.to_string().contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn claude_verifier_accepts_valid_config() {
        let config = ClaudeMergeVerifierConfig {
            api_key: "test-key".to_owned(),
            ..ClaudeMergeVerifierConfig::default()
        };
        let verifier = ClaudeMergeVerifier::new(reqwest::Client::new(), config)
            .expect("valid config must succeed");
        assert_eq!(
            verifier.messages_endpoint,
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn equivalence_prompt_contains_both_skill_texts() {
        let prompt = build_equivalence_prompt("skill alpha content", "skill beta content");
        assert!(prompt.contains("skill alpha content"));
        assert!(prompt.contains("skill beta content"));
        assert!(prompt.contains("equivalent"));
        assert!(prompt.contains("rationale"));
    }
}
