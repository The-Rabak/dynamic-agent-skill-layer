use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use domain::ExtractionError;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Simple token-bucket rate limiter for extraction HTTP endpoints.
/// Protects downstream LLM providers from accidental request floods.
struct RateLimiter {
    tokens: Mutex<f64>,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: Mutex<std::time::Instant>,
}

impl RateLimiter {
    fn new(max_requests_per_second: f64) -> Self {
        Self {
            tokens: Mutex::new(max_requests_per_second),
            max_tokens: max_requests_per_second,
            refill_rate: max_requests_per_second,
            last_refill: Mutex::new(std::time::Instant::now()),
        }
    }

    async fn acquire(&self) -> Result<(), ExtractionError> {
        loop {
            {
                let mut tokens = self.tokens.lock().unwrap();
                let mut last = self.last_refill.lock().unwrap();
                let elapsed = last.elapsed().as_secs_f64();
                *last = std::time::Instant::now();
                *tokens = (*tokens + elapsed * self.refill_rate).min(self.max_tokens);

                if *tokens >= 1.0 {
                    *tokens -= 1.0;
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

static EXTRACTION_RATE_LIMITER: LazyLock<RateLimiter> = LazyLock::new(|| RateLimiter::new(5.0));
static EXTRACTION_CLAUDE_RATE_LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(5.0));

/// Acquires one token from the shared Claude extraction rate limiter.
///
/// Exposed for adapters (e.g. the Anthropic Messages API in `claude.rs`) that
/// build their requests directly — with custom headers — instead of through
/// [`post_json`], but must still respect the same per-provider
/// request rate as the generic helper.
pub(crate) async fn acquire_claude_rate_limit() -> Result<(), ExtractionError> {
    EXTRACTION_CLAUDE_RATE_LIMITER.acquire().await
}

/// Wire shape for Ollama's `/api/generate` endpoint (non-streaming).
///
/// Used by [`ollama_generate_text`] and the internal extraction/merge paths.
#[derive(Debug, Serialize)]
pub struct OllamaGenerateTextRequest {
    pub model: String,
    pub stream: bool,
    /// Must be `"json"` to guard against thinking-model free-form output (per #190).
    pub format: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<OllamaGenerateTextOptions>,
}

/// Inference options for [`OllamaGenerateTextRequest`].
#[derive(Debug, Serialize)]
pub struct OllamaGenerateTextOptions {
    pub temperature: f32,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateTextResponse {
    response: String,
}

/// Sends one `format:"json"` prompt to Ollama's `/api/generate` and returns the
/// raw `response` field as a `String`.
///
/// This is the single Ollama-generate transport for all LLM seam impls in
/// `session-extractor`. Using one transport avoids duplicating reqwest plumbing
/// across `PreambleNormalizer`, `SkeletonLabeler`, and `SynthesisPass` impls.
///
/// ## Fail behaviour
///
/// - Connection errors → `ExtractionError::ProviderUnavailable`.
/// - Non-200 HTTP status → `ExtractionError::ProviderUnavailable`.
/// - Non-JSON response body → `ExtractionError::Unexpected`.
///
/// Callers are responsible for parsing the returned JSON string into their own
/// domain type.
pub async fn ollama_generate_text(
    client: &reqwest::Client,
    endpoint: &str,
    request: &OllamaGenerateTextRequest,
) -> Result<String, ExtractionError> {
    let raw: OllamaGenerateTextResponse =
        post_json(client, endpoint, request, "ollama-seam").await?;
    Ok(raw.response)
}

pub(crate) async fn post_json<Req, Res>(
    client: &reqwest::Client,
    endpoint: &str,
    request: &Req,
    provider_label: &str,
) -> Result<Res, ExtractionError>
where
    Req: Serialize + ?Sized,
    Res: DeserializeOwned,
{
    let rate_limiter = match provider_label {
        "claude" => &*EXTRACTION_CLAUDE_RATE_LIMITER,
        _ => &*EXTRACTION_RATE_LIMITER,
    };
    rate_limiter.acquire().await?;

    let response = client
        .post(endpoint)
        .json(request)
        .send()
        .await
        .map_err(|error| ExtractionError::ProviderUnavailable(error.to_string()))?;

    if response.status() != StatusCode::OK {
        return Err(ExtractionError::ProviderUnavailable(format!(
            "{provider_label} extraction endpoint returned {}",
            response.status()
        )));
    }

    response
        .json::<Res>()
        .await
        .map_err(|error| ExtractionError::Unexpected(error.to_string()))
}
