pub mod providers {
    pub mod claude;
    pub mod ollama;
}
pub mod transcripts;
pub mod worker_pool;
pub mod writer;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domain::{ExtractionError, ExtractionResult, TranscriptSkillExtractionService};
use infrastructure::{
    EventEnvelope, RedisStreamError, RedisStreamsAdapter, RedisStreamsConfig, RetryPolicy,
    retry_with_backoff,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use transcripts::{TranscriptError, TranscriptLoader};
use uuid::Uuid;
use worker_pool::{ExtractionWorkerPool, ExtractionWorkerPoolConfig};
use writer::{PendingDraftWriter, WriterError};

/// Request payload for the session-end extraction tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractSessionRequest {
    pub transcript_ref: String,
    #[serde(default)]
    pub transcript_inline: Option<String>,
    pub session_id: String,
    #[serde(default)]
    pub repo_path: Option<String>,
}

/// Immediate response for asynchronous extraction scheduling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractSessionResponse {
    pub status: String,
    pub reason_code: Option<String>,
    pub job_id: Option<String>,
    pub provider: Option<String>,
}

/// Captures extraction lifecycle events for in-process publishing and tests.
#[derive(Clone, Default)]
pub struct ExtractionLifecycleEvents {
    events: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl ExtractionLifecycleEvents {
    /// Records a lifecycle event envelope.
    pub fn push(&self, envelope: EventEnvelope) {
        if let Ok(mut lock) = self.events.lock() {
            lock.push(envelope);
        }
    }

    /// Returns a snapshot of currently recorded lifecycle events.
    pub fn list(&self) -> Vec<EventEnvelope> {
        self.events
            .lock()
            .map(|lock| lock.clone())
            .unwrap_or_default()
    }
}

/// Runtime seam for publishing extraction lifecycle events to the shared bus.
#[async_trait]
pub trait ExtractionEventPublisher: Send + Sync {
    /// Publishes one lifecycle event envelope.
    async fn publish(&self, envelope: &EventEnvelope) -> Result<(), LifecycleEventPublishError>;
}

/// Explicit publication failure surfaced by lifecycle event publisher adapters.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("lifecycle event publication failed: {message}")]
pub struct LifecycleEventPublishError {
    message: String,
}

impl LifecycleEventPublishError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<RedisStreamError> for LifecycleEventPublishError {
    fn from(error: RedisStreamError) -> Self {
        Self::new(error.to_string())
    }
}

/// Redis Streams adapter for runtime extraction lifecycle event publication.
#[derive(Clone)]
pub struct RedisExtractionEventPublisher {
    stream_adapter: RedisStreamsAdapter,
}

impl RedisExtractionEventPublisher {
    /// Builds the Redis publisher using shared stream config environment variables.
    pub fn from_environment() -> Result<Self, SessionExtractorInitError> {
        let mut stream_config = RedisStreamsConfig::default();
        if let Ok(redis_url) = std::env::var("REDIS_URL")
            && !redis_url.trim().is_empty()
        {
            stream_config.redis_url = redis_url;
        }
        if let Ok(stream_key) = std::env::var("REDIS_STREAM_KEY")
            && !stream_key.trim().is_empty()
        {
            stream_config.stream_key = stream_key;
        }
        if let Ok(consumer_group) = std::env::var("REDIS_CONSUMER_GROUP")
            && !consumer_group.trim().is_empty()
        {
            stream_config.consumer_group = consumer_group;
        }
        if let Ok(consumer_name) = std::env::var("REDIS_CONSUMER_NAME")
            && !consumer_name.trim().is_empty()
        {
            stream_config.consumer_name = consumer_name;
        }

        Ok(Self {
            stream_adapter: RedisStreamsAdapter::new(stream_config)?,
        })
    }
}

#[async_trait]
impl ExtractionEventPublisher for RedisExtractionEventPublisher {
    async fn publish(&self, envelope: &EventEnvelope) -> Result<(), LifecycleEventPublishError> {
        self.stream_adapter
            .publish(envelope)
            .await
            .map(|_| ())
            .map_err(LifecycleEventPublishError::from)
    }
}

#[derive(Clone, Default)]
pub(crate) struct NoopExtractionEventPublisher;

#[async_trait]
impl ExtractionEventPublisher for NoopExtractionEventPublisher {
    async fn publish(&self, _envelope: &EventEnvelope) -> Result<(), LifecycleEventPublishError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionProvider {
    Claude,
    Ollama,
}

impl ExtractionProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Ollama => "ollama",
        }
    }
}

impl std::str::FromStr for ExtractionProvider {
    type Err = SessionExtractorInitError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "claude" => Ok(Self::Claude),
            "ollama" => Ok(Self::Ollama),
            other => Err(SessionExtractorInitError::InvalidProvider(other.to_owned())),
        }
    }
}

/// Coordinates transcript loading, provider extraction, draft writing, and lifecycle events.
#[derive(Clone)]
pub struct SessionExtractor {
    pub(crate) provider: ExtractionProvider,
    pub(crate) extractor: Arc<dyn TranscriptSkillExtractionService>,
    pub(crate) transcript_loader: TranscriptLoader,
    pub(crate) draft_writer: PendingDraftWriter,
    pub(crate) lifecycle_events: ExtractionLifecycleEvents,
    pub(crate) event_publisher: Arc<dyn ExtractionEventPublisher>,
    pub(crate) worker_pool: Option<ExtractionWorkerPool>,
}

impl SessionExtractor {
    /// Constructs the default extractor runtime from environment-driven routing.
    pub fn from_environment() -> Result<Self, SessionExtractorInitError> {
        let provider = std::env::var("EXTRACT_SESSION_PROVIDER")
            .unwrap_or_else(|_| "claude".to_owned())
            .parse::<ExtractionProvider>()?;
        let extractor = match provider {
            ExtractionProvider::Claude => {
                providers::claude::build_extractor(reqwest::Client::new())?
            }
            ExtractionProvider::Ollama => {
                providers::ollama::build_extractor(reqwest::Client::new())?
            }
        };

        Ok(Self {
            provider,
            extractor,
            transcript_loader: TranscriptLoader::from_environment()?,
            draft_writer: PendingDraftWriter::from_environment()?,
            lifecycle_events: ExtractionLifecycleEvents::default(),
            event_publisher: Arc::new(RedisExtractionEventPublisher::from_environment()?),
            worker_pool: Some(ExtractionWorkerPool::new(
                ExtractionWorkerPoolConfig::default(),
            )),
        })
    }

    /// Constructs a fully-injected extractor for deterministic tests.
    pub fn new_for_tests(
        provider: ExtractionProvider,
        extractor: Arc<dyn TranscriptSkillExtractionService>,
        transcript_loader: TranscriptLoader,
        draft_writer: PendingDraftWriter,
    ) -> Self {
        Self::new_for_tests_with_publisher(
            provider,
            extractor,
            transcript_loader,
            draft_writer,
            Arc::new(NoopExtractionEventPublisher),
        )
    }

    /// Constructs a fully-injected extractor and lifecycle publisher for tests.
    pub fn new_for_tests_with_publisher(
        provider: ExtractionProvider,
        extractor: Arc<dyn TranscriptSkillExtractionService>,
        transcript_loader: TranscriptLoader,
        draft_writer: PendingDraftWriter,
        event_publisher: Arc<dyn ExtractionEventPublisher>,
    ) -> Self {
        Self {
            provider,
            extractor,
            transcript_loader,
            draft_writer,
            lifecycle_events: ExtractionLifecycleEvents::default(),
            event_publisher,
            worker_pool: Some(ExtractionWorkerPool::new(
                ExtractionWorkerPoolConfig::default(),
            )),
        }
    }

    /// Constructs an extractor with an explicit worker pool config for tests.
    pub fn new_for_tests_with_pool(
        provider: ExtractionProvider,
        extractor: Arc<dyn TranscriptSkillExtractionService>,
        transcript_loader: TranscriptLoader,
        draft_writer: PendingDraftWriter,
        event_publisher: Arc<dyn ExtractionEventPublisher>,
        pool_config: ExtractionWorkerPoolConfig,
    ) -> Self {
        Self {
            provider,
            extractor,
            transcript_loader,
            draft_writer,
            lifecycle_events: ExtractionLifecycleEvents::default(),
            event_publisher,
            worker_pool: Some(ExtractionWorkerPool::new(pool_config)),
        }
    }

    /// Returns lifecycle events emitted by accepted extraction jobs.
    pub fn lifecycle_events(&self) -> Vec<EventEnvelope> {
        self.lifecycle_events.list()
    }

    /// Schedules asynchronous extraction after transcript contract validation.
    /// Uses worker pool when available; falls back to direct spawn.
    pub async fn enqueue(&self, request: ExtractSessionRequest) -> ExtractSessionResponse {
        if request.transcript_inline.is_none()
            && let Err(error) = self.transcript_loader.validate_ref(&request.transcript_ref)
        {
            return ExtractSessionResponse {
                status: "failed".to_owned(),
                reason_code: Some(error.reason_code()),
                job_id: None,
                provider: None,
            };
        }
        if let Err(error) = self.draft_writer.validate_scope_root(&request) {
            return ExtractSessionResponse {
                status: "failed".to_owned(),
                reason_code: Some(error.reason_code()),
                job_id: None,
                provider: None,
            };
        }

        let job_id = Uuid::now_v7().to_string();
        let requested_event = EventEnvelope::new(
            "skill.extraction_requested",
            format!("skill.extraction_requested:{job_id}"),
            json!({
                "job_id": job_id.as_str(),
                "provider": self.provider.as_str(),
                "session_id": request.session_id.as_str(),
                "transcript_ref": request.transcript_ref.as_str(),
            }),
        );
        if let Err(error) = self.publish_lifecycle_event(requested_event).await {
            return ExtractSessionResponse {
                status: "failed".to_owned(),
                reason_code: Some(error.reason_code().to_owned()),
                job_id: None,
                provider: None,
            };
        }

        if let Some(ref pool) = self.worker_pool {
            match pool.submit(self.clone(), job_id.clone(), request.clone()) {
                Ok(_response_rx) => ExtractSessionResponse {
                    status: "processing".to_owned(),
                    reason_code: None,
                    job_id: Some(job_id),
                    provider: Some(self.provider.as_str().to_owned()),
                },
                Err(rejection) => rejection,
            }
        } else {
            let worker = self.clone();
            let worker_request = request;
            let worker_job_id = job_id.clone();
            tokio::spawn(async move {
                if let Err(error) = worker.execute_job(&worker_job_id, &worker_request).await {
                    if let Err(failed_publish_error) = worker
                        .publish_lifecycle_event(EventEnvelope::new(
                            "extraction.failed",
                            format!("extraction.failed:{worker_job_id}"),
                            json!({
                                "job_id": worker_job_id.as_str(),
                                "provider": worker.provider.as_str(),
                                "error": error.to_string(),
                            }),
                        ))
                        .await
                    {
                        eprintln!(
                            "failed to publish extraction.failed lifecycle event: {}",
                            failed_publish_error
                        );
                    }
                }
            });

            ExtractSessionResponse {
                status: "processing".to_owned(),
                reason_code: None,
                job_id: Some(job_id),
                provider: Some(self.provider.as_str().to_owned()),
            }
        }
    }

    pub(crate) async fn execute_job(
        &self,
        job_id: &str,
        request: &ExtractSessionRequest,
    ) -> Result<(), SessionExtractionError> {
        let transcript = self.transcript_loader.load(
            &request.session_id,
            &request.transcript_ref,
            request.transcript_inline.as_deref(),
        )?;
        let extraction_result = self.extract_with_retry(&transcript).await?;
        let draft_paths = self.draft_writer.write_pending_drafts(
            &extraction_result,
            request,
            self.provider.as_str(),
        )?;

        self.publish_lifecycle_event(EventEnvelope::new(
            "extraction.completed",
            format!("extraction.completed:{job_id}"),
            json!({
                "job_id": job_id,
                "provider": self.provider.as_str(),
                "source_session_id": extraction_result.source_session_id.as_str(),
                "draft_count": draft_paths.len(),
                "draft_paths": draft_paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            }),
        ))
        .await?;

        Ok(())
    }

    async fn extract_with_retry(
        &self,
        transcript: &domain::SessionTranscript,
    ) -> Result<ExtractionResult, SessionExtractionError> {
        let policy = RetryPolicy {
            max_attempts: 3,
            base_delay: std::time::Duration::from_millis(150),
            max_delay: std::time::Duration::from_secs(1),
        };

        retry_with_backoff(&policy, || {
            let extractor = Arc::clone(&self.extractor);
            async move { extractor.extract(transcript).await }
        })
        .await
        .map_err(SessionExtractionError::from)
    }

    pub(crate) async fn publish_lifecycle_event(
        &self,
        envelope: EventEnvelope,
    ) -> Result<(), SessionExtractionError> {
        self.lifecycle_events.push(envelope.clone());
        self.event_publisher.publish(&envelope).await?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SessionExtractorInitError {
    #[error("unsupported extraction provider `{0}`")]
    InvalidProvider(String),
    #[error(transparent)]
    Transcript(#[from] TranscriptError),
    #[error(transparent)]
    Writer(#[from] WriterError),
    #[error(transparent)]
    Extraction(#[from] ExtractionError),
    #[error(transparent)]
    Redis(#[from] RedisStreamError),
}

#[derive(Debug, Error)]
pub(crate) enum SessionExtractionError {
    #[error(transparent)]
    Transcript(#[from] TranscriptError),
    #[error(transparent)]
    Extraction(#[from] ExtractionError),
    #[error(transparent)]
    Writer(#[from] WriterError),
    #[error(transparent)]
    EventPublication(#[from] LifecycleEventPublishError),
}

impl SessionExtractionError {
    fn reason_code(&self) -> &'static str {
        match self {
            Self::Transcript(error) => match error {
                TranscriptError::InvalidRoot(_) => "invalid_transcript_root",
                TranscriptError::InvalidReference(_) => "invalid_transcript_ref",
                TranscriptError::ReadFailure(_, _) => "transcript_read_failed",
                TranscriptError::InvalidPayload(_) => "invalid_transcript_payload",
            },
            Self::Extraction(_) => "extraction_failed",
            Self::Writer(error) => match error {
                WriterError::InvalidRepoPath(_) => "invalid_repo_path",
                WriterError::ScopeResolution(_) => "scope_resolution_failed",
                WriterError::WriteFailure(_, _) => "pending_draft_write_failed",
                WriterError::FrontmatterSerialization(_) => {
                    "pending_frontmatter_serialization_failed"
                }
                WriterError::BatchValidation(_) => "pending_draft_batch_validation_failed",
                WriterError::RejectedTombstonePresent(_) => "rejected_tombstone_present",
                WriterError::WriteDenied(_) => "write_denied",
            },
            Self::EventPublication(_) => "event_publication_failed",
        }
    }
}

/// Normalizes provider outputs for writer and event paths.
pub fn extraction_contract_view(result: &ExtractionResult) -> serde_json::Value {
    json!({
        "source_session_id": result.source_session_id.as_str(),
        "provider": result.provider,
        "candidates": result.candidates.iter().map(|candidate| {
            json!({
                "name": candidate.name,
                "description": candidate.description,
                "tags": candidate.tags,
                "procedures": candidate.procedures,
                "conventions": candidate.conventions,
                "assets": candidate.assets,
                "confidence": candidate.confidence,
            })
        }).collect::<Vec<_>>(),
    })
}