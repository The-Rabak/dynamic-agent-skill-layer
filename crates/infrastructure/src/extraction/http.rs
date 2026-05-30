use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use domain::ExtractionError;
use reqwest::StatusCode;
use serde::{Serialize, de::DeserializeOwned};
use tokio::time::timeout;

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

static EXTRACTION_RATE_LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(5.0));
static EXTRACTION_CLAUDE_RATE_LIMITER: LazyLock<RateLimiter> =
    LazyLock::new(|| RateLimiter::new(5.0));

pub(crate) async fn post_json_with_timeout<Req, Res>(
    client: &reqwest::Client,
    endpoint: &str,
    request: &Req,
    timeout_ms: u64,
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

    timeout(Duration::from_millis(timeout_ms), async {
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
    })
    .await
    .map_err(|_| ExtractionError::Timeout { timeout_ms })
    .and_then(|result| result)
}