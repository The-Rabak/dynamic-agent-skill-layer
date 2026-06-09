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

/// The Ollama context window (`num_ctx`, in tokens) sent on EVERY `/api/generate`
/// request across the whole orchestrated extraction path.
///
/// **Why this exists (the #176/#214 root cause):** Ollama's built-in default
/// context is only ~4096 tokens. We never used to send `num_ctx`, so when an
/// orchestration window's prompt exceeded 4096 tokens Ollama SILENTLY truncated the
/// input (`n_keep` drops all but the first few tokens — including the JSON-contract
/// instructions). The model then emitted malformed / keyless JSON, which parsed to
/// zero candidates after retries. This bit precisely the *substantive* windows
/// (dense with kept prose) — the skill-rich ones — while sparse/tool-heavy windows
/// shrank under sanitization and slipped in. Confirmed from the model server's own
/// logs (`n_ctx_slot = 4096, n_keep = 4, truncated = 1`).
///
/// **The alignment invariant (do not break):** every size lever on the local/Ollama
/// path must fit inside this context so content that fits a chunk is never silently
/// truncated:
///   `LOCAL_TIER_TOKEN_BUDGET (8 192, window content)` + mined preamble + prompt
///   scaffold  ≤  `EXTRACTION_OLLAMA_NUM_CTX (16 384)`,  with the remainder left for
///   the model's JSON output. 16 384 gives ~2× headroom over the 8 192-token window.
/// Frontier windows (40 960) only ever route to claude/claude-code, which do not use
/// `num_ctx`, so they never reach an Ollama context.
///
/// Override via `OLLAMA_NUM_CTX` (fail-loud on a non-integer value).
pub const EXTRACTION_OLLAMA_NUM_CTX: u32 = 16_384;

/// Resolves the Ollama `num_ctx` to send: `OLLAMA_NUM_CTX` if set (fail-loud on a
/// non-integer or empty-after-trim value), otherwise [`EXTRACTION_OLLAMA_NUM_CTX`].
/// Read per request — cheap, and lets an operator retune without a rebuild.
pub fn extraction_ollama_num_ctx() -> u32 {
    match std::env::var("OLLAMA_NUM_CTX") {
        Ok(raw) if !raw.trim().is_empty() => raw.trim().parse().unwrap_or_else(|error| {
            panic!("invalid OLLAMA_NUM_CTX value {raw:?}: {error} (must be a positive integer)")
        }),
        _ => EXTRACTION_OLLAMA_NUM_CTX,
    }
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
    /// Disables the model's "thinking" mode (#176). gemma4:12b is a thinking model:
    /// even with `format:"json"` it otherwise emits its chain-of-thought AS JSON
    /// keys instead of the contracted shape, which downstream parsers then read as
    /// empty/malformed. `format` forces valid JSON syntax but does not stop the
    /// reasoning leaking into the keys; `think:false` does. Set `false` for every
    /// structured seam call (preamble/skeleton/synthesis).
    pub think: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<OllamaGenerateTextOptions>,
}

/// Inference options for [`OllamaGenerateTextRequest`].
#[derive(Debug, Serialize)]
pub struct OllamaGenerateTextOptions {
    /// Context window in tokens — ALWAYS set to prevent silent input truncation
    /// (see [`EXTRACTION_OLLAMA_NUM_CTX`] / #176 / #214).
    pub num_ctx: u32,
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
