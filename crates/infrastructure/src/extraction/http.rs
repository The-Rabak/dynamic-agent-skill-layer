use std::time::Duration;

use domain::ExtractionError;
use reqwest::StatusCode;
use serde::{Serialize, de::DeserializeOwned};
use tokio::time::timeout;

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
