use std::sync::{Arc, Mutex};

use infrastructure::EventEnvelope;
use serde::{Deserialize, Serialize};
use session_extractor::{
    ExtractSessionRequest as SessionExtractorRequest, ExtractSessionResponse, SessionExtractor,
};

/// MCP-facing extraction request envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractSessionRequest {
    pub transcript_ref: String,
    #[serde(default)]
    pub transcript_inline: Option<String>,
    pub session_id: String,
    #[serde(default)]
    pub repo_path: Option<String>,
}

/// Thin transport adapter that delegates session extraction to `session-extractor`.
#[derive(Clone)]
pub struct ExtractSessionTool {
    runtime: Arc<Mutex<Option<Result<SessionExtractor, String>>>>,
}

impl ExtractSessionTool {
    pub fn from_environment() -> Self {
        Self {
            runtime: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn new_for_tests(extractor: SessionExtractor) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(Some(Ok(extractor)))),
        }
    }

    pub fn lifecycle_events(&self) -> Vec<EventEnvelope> {
        self.with_runtime(|runtime| runtime.lifecycle_events())
            .unwrap_or_default()
    }

    pub async fn invoke(&self, request: ExtractSessionRequest) -> ExtractSessionResponse {
        let runtime = match self.with_runtime(Clone::clone) {
            Ok(runtime) => runtime,
            Err(error) => {
                return ExtractSessionResponse {
                    status: "failed".to_owned(),
                    reason_code: Some(format!("extract_session_unavailable:{error}")),
                    job_id: None,
                    provider: None,
                };
            }
        };

        runtime
            .enqueue(SessionExtractorRequest {
                transcript_ref: request.transcript_ref,
                transcript_inline: request.transcript_inline,
                session_id: request.session_id,
                repo_path: request.repo_path,
            })
            .await
    }

    fn with_runtime<T>(&self, map: impl FnOnce(&SessionExtractor) -> T) -> Result<T, String> {
        let mut lock = self
            .runtime
            .lock()
            .map_err(|_| "extract_session runtime lock poisoned".to_owned())?;
        if lock.is_none() {
            *lock = Some(SessionExtractor::from_environment().map_err(|error| error.to_string()));
        }

        let runtime = lock
            .as_ref()
            .ok_or_else(|| "extract_session runtime not initialized".to_owned())?
            .as_ref()
            .map_err(Clone::clone)?;

        Ok(map(runtime))
    }
}
