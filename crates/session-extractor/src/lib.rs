// FOLLOW-UP (post-v1.5): split this module — see todo #128. Intended split:
// `extraction_job.rs` (SessionExtractor + enqueue/run), `events.rs`
// (ExtractionLifecycleEvents + event types), `provider_dispatch.rs`
// (ExtractionProvider + from_environment).
pub mod providers {
    pub mod claude;
    pub mod claude_code;
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

/// Selects the extraction provider backend.
///
/// The `EXTRACT_SESSION_PROVIDER` env variable controls which variant is chosen:
/// - unset / blank / `"ollama"` → `Ollama` (local default; constitution v2.0.0)
/// - `"claude"` / `"claude-api"` → `Claude` (Anthropic Messages API, requires `ANTHROPIC_API_KEY`;
///   fails loudly at construction when the key is absent — Constitution Principle 1)
/// - `"claude-code"` / `"claude-cli"` → `ClaudeCode` (Claude Code CLI subscription, host-only;
///   CLI absence surfaces at first extraction via `ProviderUnavailable`, no silent fallback)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionProvider {
    /// Anthropic Messages API path (`EXTRACT_SESSION_PROVIDER=claude` or `=claude-api`).
    /// Requires `ANTHROPIC_API_KEY` — missing key fails loudly at construct time
    /// (Constitution Principle 1: no silent cloud attempt, no silent fallback).
    Claude,
    /// Claude Code CLI subscription path (`EXTRACT_SESSION_PROVIDER=claude-code` or `=claude-cli`).
    /// Host-only — not suitable for containerised deployments without CLI mount.
    /// CLI absence surfaces at first extraction via `ExtractionError::ProviderUnavailable`.
    ClaudeCode,
    Ollama,
}

impl ExtractionProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Claude => "claude",
            Self::Ollama => "ollama",
        }
    }
}

impl std::str::FromStr for ExtractionProvider {
    type Err = SessionExtractorInitError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        // Ollama is the default local provider (constitution v2.0.0): an unset or
        // blank `EXTRACT_SESSION_PROVIDER` selects Ollama, never a cloud path.
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "ollama" => Ok(Self::Ollama),
            // "claude" / "claude-api" → Anthropic Messages API (requires ANTHROPIC_API_KEY).
            // "claude-api" is accepted as an alias so existing deployments using the prior
            // selector continue to work without reconfiguration.
            "claude" | "claude-api" => Ok(Self::Claude),
            // "claude-code" / "claude-cli" → Claude Code CLI subscription (host-only, no API key).
            "claude-code" | "claude-cli" => Ok(Self::ClaudeCode),
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
    /// Single config-sourced retry policy applied to the provider extraction
    /// call. Both the no-pool path and the worker loop share this value (the
    /// worker pool clones the whole extractor into each job), so retry behavior
    /// is sourced from exactly one place.
    pub(crate) retry_policy: RetryPolicy,
    /// Per-job extraction timeout, sourced from the same pool config as the
    /// worker loop. Carried on the extractor so the no-pool spawn path can apply
    /// its own timeout arm (otherwise a slow extraction stalls silently).
    pub(crate) job_timeout: std::time::Duration,
}

/// Typed result of one extraction job, produced by [`SessionExtractor::execute_job`].
///
/// `execute_job` is a pure outcome producer: it publishes NO lifecycle events.
/// Terminal-event ownership (`extraction.completed` / `extraction.failed`) lives
/// entirely in the dispatch layer — the worker loop, the no-pool spawn path, and
/// [`SessionExtractor::extract_blocking`] — so every accepted job emits exactly
/// one terminal event regardless of dispatch route.
pub(crate) enum ExtractionOutcome {
    /// Extraction succeeded; carries the committed `.pending` draft paths and the
    /// source session id for the `extraction.completed` payload.
    Completed {
        draft_paths: Vec<std::path::PathBuf>,
        source_session_id: String,
    },
    /// Extraction failed at some stage; carries the typed error for reason-code
    /// mapping and the `extraction.failed` payload.
    Failed(SessionExtractionError),
}

impl SessionExtractor {
    /// Constructs the default extractor runtime from environment-driven routing.
    pub fn from_environment() -> Result<Self, SessionExtractorInitError> {
        let provider = std::env::var("EXTRACT_SESSION_PROVIDER")
            .unwrap_or_default()
            .parse::<ExtractionProvider>()?;
        let extractor = match provider {
            // Anthropic Messages API path: requires ANTHROPIC_API_KEY.
            // Fails loudly at construction when the key is absent (Constitution Principle 1).
            ExtractionProvider::Claude => {
                providers::claude::build_extractor(reqwest::Client::new())?
            }
            // Claude Code CLI subscription path: host-only, no API key.
            ExtractionProvider::ClaudeCode => providers::claude_code::build_extractor()?,
            // Local default (constitution v2.0.0).
            ExtractionProvider::Ollama => {
                providers::ollama::build_extractor(reqwest::Client::new())?
            }
        };

        let pool_config = ExtractionWorkerPoolConfig::default();
        let retry_policy = pool_config.retry_policy.clone();
        let job_timeout = pool_config.timeout;
        Ok(Self {
            provider,
            extractor,
            transcript_loader: TranscriptLoader::from_environment()?,
            draft_writer: PendingDraftWriter::from_environment()?,
            lifecycle_events: ExtractionLifecycleEvents::default(),
            event_publisher: Arc::new(RedisExtractionEventPublisher::from_environment()?),
            worker_pool: Some(ExtractionWorkerPool::new(pool_config)),
            retry_policy,
            job_timeout,
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
        let pool_config = ExtractionWorkerPoolConfig::default();
        let retry_policy = pool_config.retry_policy.clone();
        let job_timeout = pool_config.timeout;
        Self {
            provider,
            extractor,
            transcript_loader,
            draft_writer,
            lifecycle_events: ExtractionLifecycleEvents::default(),
            event_publisher,
            worker_pool: Some(ExtractionWorkerPool::new(pool_config)),
            retry_policy,
            job_timeout,
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
                // The no-pool spawn path owns terminal events itself, including a
                // timeout arm — otherwise a slow extraction stalls silently.
                let outcome = match tokio::time::timeout(
                    worker.job_timeout,
                    worker.execute_job(&worker_job_id, &worker_request),
                )
                .await
                {
                    Ok(outcome) => outcome,
                    Err(_elapsed) => {
                        worker.publish_timeout_event(&worker_job_id).await;
                        return;
                    }
                };
                worker.publish_terminal_event(&worker_job_id, outcome).await;
            });

            ExtractSessionResponse {
                status: "processing".to_owned(),
                reason_code: None,
                job_id: Some(job_id),
                provider: Some(self.provider.as_str().to_owned()),
            }
        }
    }

    /// Runs one extraction job to a typed [`ExtractionOutcome`], publishing NO
    /// lifecycle events. Terminal-event ownership belongs to the dispatch layer
    /// (worker loop, no-pool spawn path, [`Self::extract_blocking`]).
    pub(crate) async fn execute_job(
        &self,
        _job_id: &str,
        request: &ExtractSessionRequest,
    ) -> ExtractionOutcome {
        let transcript = match self.transcript_loader.load(
            &request.session_id,
            &request.transcript_ref,
            request.transcript_inline.as_deref(),
        ) {
            Ok(transcript) => transcript,
            Err(error) => return ExtractionOutcome::Failed(error.into()),
        };
        let extraction_result = match self.extract_with_retry(&transcript).await {
            Ok(result) => result,
            Err(error) => return ExtractionOutcome::Failed(error),
        };
        let draft_paths = match self.draft_writer.write_pending_drafts(
            &extraction_result,
            request,
            self.provider.as_str(),
        ) {
            Ok(paths) => paths,
            Err(error) => return ExtractionOutcome::Failed(error.into()),
        };

        ExtractionOutcome::Completed {
            draft_paths,
            source_session_id: extraction_result.source_session_id.as_str().to_owned(),
        }
    }

    /// Publishes exactly one terminal lifecycle event for a finished job.
    ///
    /// `Completed` maps to `extraction.completed` (with scope-relative draft
    /// paths — never absolute host paths); `Failed` maps to `extraction.failed`.
    /// This is the single terminal-event publication site shared by the worker
    /// loop and the no-pool spawn path.
    pub(crate) async fn publish_terminal_event(&self, job_id: &str, outcome: ExtractionOutcome) {
        let envelope = match outcome {
            ExtractionOutcome::Completed {
                draft_paths,
                source_session_id,
            } => EventEnvelope::new(
                "extraction.completed",
                format!("extraction.completed:{job_id}"),
                json!({
                    "job_id": job_id,
                    "provider": self.provider.as_str(),
                    "source_session_id": source_session_id,
                    "draft_count": draft_paths.len(),
                    "draft_paths": self.scope_relative_draft_paths(&draft_paths),
                }),
            ),
            ExtractionOutcome::Failed(error) => EventEnvelope::new(
                "extraction.failed",
                format!("extraction.failed:{job_id}"),
                json!({
                    "job_id": job_id,
                    "provider": self.provider.as_str(),
                    "reason_code": error.reason_code(),
                    "error": error.to_string(),
                }),
            ),
        };
        if let Err(publish_error) = self.publish_lifecycle_event(envelope).await {
            tracing::error!(
                ?publish_error,
                "failed to publish terminal extraction lifecycle event"
            );
        }
    }

    /// Publishes the timeout terminal event. Timeout maps to `extraction.failed`
    /// with the canonical "extraction timed out" error string (the Redis 8-event
    /// catalog is frozen — there is no dedicated `extraction.timeout` type).
    pub(crate) async fn publish_timeout_event(&self, job_id: &str) {
        if let Err(publish_error) = self
            .publish_lifecycle_event(EventEnvelope::new(
                "extraction.failed",
                format!("extraction.failed:{job_id}"),
                json!({
                    "job_id": job_id,
                    "provider": self.provider.as_str(),
                    "error": "extraction timed out",
                }),
            ))
            .await
        {
            tracing::error!(
                ?publish_error,
                "failed to publish timeout extraction.failed lifecycle event"
            );
        }
    }

    /// Renders committed draft paths as scope-relative strings for event payloads.
    ///
    /// Security P1: the `extraction.completed` event must never leak absolute
    /// host paths. Committed paths live under `<scope_root>/.skills/...`, so we
    /// emit each path relative to its scope root (falling back to the `.skills`
    /// suffix if the prefix cannot be stripped). The returned `.pending` paths
    /// (used internally for `draft_count`) remain absolute.
    fn scope_relative_draft_paths(&self, draft_paths: &[std::path::PathBuf]) -> Vec<String> {
        draft_paths
            .iter()
            .map(|path| relative_draft_path(path))
            .collect()
    }

    /// Runs one extraction synchronously and returns the written `.pending`
    /// paths, awaiting completion instead of dispatching to the worker pool.
    ///
    /// This is the entry point for the durable transcript-ingest queue drain
    /// (todo 103): the maintenance worker feeds queued transcript *content* via
    /// `transcript_inline` and marks the queue row `processed` only after this
    /// returns `Ok` — so a draft is durably written before the work is acked.
    /// The path validator is never exercised on the inline path, which is why
    /// the absolute-`{{transcript_path}}` bug is moot for this flow.
    ///
    /// Errors are returned as stable reason-code strings suitable for the
    /// queue's `error` column.
    pub async fn extract_blocking(
        &self,
        request: &ExtractSessionRequest,
    ) -> Result<Vec<std::path::PathBuf>, String> {
        if request.transcript_inline.is_none()
            && let Err(error) = self.transcript_loader.validate_ref(&request.transcript_ref)
        {
            return Err(error.reason_code());
        }
        self.draft_writer
            .validate_scope_root(request)
            .map_err(|error| error.reason_code())?;

        let job_id = Uuid::now_v7().to_string();
        // The blocking drain owns its terminal events itself: it must emit
        // `extraction.completed` on success so the queue drain observes
        // completion before acking a row (todo 103 coupling), and
        // `extraction.failed` on error for operator visibility.
        match self.execute_job(&job_id, request).await {
            ExtractionOutcome::Completed {
                draft_paths,
                source_session_id,
            } => {
                self.publish_terminal_event(
                    &job_id,
                    ExtractionOutcome::Completed {
                        draft_paths: draft_paths.clone(),
                        source_session_id,
                    },
                )
                .await;
                Ok(draft_paths)
            }
            ExtractionOutcome::Failed(error) => {
                let reason_code = error.reason_code();
                let detail = error.to_string();
                self.publish_terminal_event(&job_id, ExtractionOutcome::Failed(error))
                    .await;
                // Carry the reason code AND the underlying detail so the queue's
                // `error` column is actionable for operators, not just a bare
                // category. Format: `<reason_code>: <detail>`.
                Err(format!("{reason_code}: {detail}"))
            }
        }
    }

    async fn extract_with_retry(
        &self,
        transcript: &domain::SessionTranscript,
    ) -> Result<ExtractionResult, SessionExtractionError> {
        retry_with_backoff(&self.retry_policy, || {
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
            Self::Extraction(error) => match error {
                ExtractionError::ProviderUnavailable(_) => "provider_unavailable",
                _ => "extraction_failed",
            },
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

/// Renders one committed draft path as a scope-relative string.
///
/// Committed `.pending` drafts live under `<scope_root>/.skills/...`. To keep
/// absolute host paths out of the published `extraction.completed` event
/// (Security P1), we emit the path starting at the `.skills` component. If no
/// `.skills` component is present (unexpected), we fall back to the final path
/// component, never the absolute path.
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

fn relative_draft_path(path: &std::path::Path) -> String {
    use std::path::Component;

    let mut found_skills = false;
    let mut relative = std::path::PathBuf::new();
    for component in path.components() {
        if let Component::Normal(name) = component
            && name == std::ffi::OsStr::new(".skills")
        {
            found_skills = true;
        }
        if found_skills {
            relative.push(component);
        }
    }

    if found_skills {
        relative.display().to_string()
    } else {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        str::FromStr,
        sync::atomic::{AtomicU32, Ordering},
        time::Duration,
    };

    use async_trait::async_trait;
    use domain::{
        DomainId, ExtractedSkillCandidate, ExtractionError, ExtractionResult, SessionTranscript,
        TranscriptSkillExtractionService,
    };

    use super::*;
    use crate::{transcripts::TranscriptLoader, writer::PendingDraftWriter};

    #[test]
    fn provider_from_str_maps_per_dispatch_contract() {
        // Unset / blank / "ollama" → Ollama (constitution v2.0.0 default).
        assert_eq!(
            ExtractionProvider::from_str("").expect("empty parses"),
            ExtractionProvider::Ollama,
            "empty EXTRACT_SESSION_PROVIDER must map to Ollama, not a cloud path"
        );
        assert_eq!(
            ExtractionProvider::from_str("   ").expect("whitespace parses"),
            ExtractionProvider::Ollama,
        );
        assert_eq!(
            ExtractionProvider::from_str("ollama").expect("ollama parses"),
            ExtractionProvider::Ollama,
        );

        // "claude" → Anthropic Messages API (Claude), requires ANTHROPIC_API_KEY.
        // Constitution Principle 1: selecting this without a key fails loudly at construction.
        assert_eq!(
            ExtractionProvider::from_str("claude").expect("claude parses"),
            ExtractionProvider::Claude,
            "'claude' must map to the Anthropic Messages API provider"
        );
        // "claude-api" is an accepted alias for Claude (backwards-compat for prior deployments).
        assert_eq!(
            ExtractionProvider::from_str("claude-api").expect("claude-api parses"),
            ExtractionProvider::Claude,
        );

        // "claude-code" / "claude-cli" → Claude Code CLI subscription path.
        assert_eq!(
            ExtractionProvider::from_str("claude-code").expect("claude-code parses"),
            ExtractionProvider::ClaudeCode,
        );
        assert_eq!(
            ExtractionProvider::from_str("claude-cli").expect("claude-cli parses"),
            ExtractionProvider::ClaudeCode,
        );

        // Unknown provider strings must be a loud error (no silent fallback).
        assert!(
            ExtractionProvider::from_str("gpt").is_err(),
            "unknown provider must be a loud error"
        );
        assert!(
            ExtractionProvider::from_str("openai").is_err(),
            "unknown provider must be a loud error"
        );
    }

    #[test]
    fn provider_from_str_as_str_round_trips() {
        // Every as_str() output must be accepted by from_str() and produce the same variant.
        for variant in [
            ExtractionProvider::Claude,
            ExtractionProvider::ClaudeCode,
            ExtractionProvider::Ollama,
        ] {
            let serialized = variant.as_str();
            let deserialized = ExtractionProvider::from_str(serialized).unwrap_or_else(|_| {
                panic!("as_str() output '{serialized}' must round-trip through from_str()")
            });
            assert_eq!(
                deserialized, variant,
                "from_str(as_str({variant:?})) must equal {variant:?}"
            );
        }
    }

    /// Verifies that selecting `EXTRACT_SESSION_PROVIDER=claude` (the Anthropic Messages API
    /// path) without `ANTHROPIC_API_KEY` fails loudly at construction — never silently.
    ///
    /// This is the env-route version of the test in `claude.rs`; it exercises the full
    /// `from_str` dispatch so the Constitution Principle 1 guarantee is proven end-to-end
    /// for the `=claude` selector, not just for `ClaudeExtractor::new` in isolation.
    #[test]
    fn env_route_claude_without_api_key_fails_loudly_at_construction() {
        // Guard: ensure ANTHROPIC_API_KEY is absent for this test.
        let _guard = std::env::var("ANTHROPIC_API_KEY").ok();
        // SAFETY: single-threaded test; we restore the value (or remove it) after the test.
        // Using remove_var rather than set_var to an empty string because build_extractor
        // reads the raw env var (absent ≠ blank for some callers).
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
        }

        let provider = ExtractionProvider::from_str("claude").expect("'claude' must parse");
        assert_eq!(
            provider,
            ExtractionProvider::Claude,
            "'claude' must route to the Anthropic Messages API provider"
        );

        // Constructing the extractor via the real provider builder must fail loudly
        // at construction (not at extraction time) when ANTHROPIC_API_KEY is missing.
        let error = crate::providers::claude::build_extractor(reqwest::Client::new())
            .err()
            .expect("building the Claude provider without ANTHROPIC_API_KEY must fail loudly at construction");
        assert!(
            matches!(error, domain::ExtractionError::ProviderUnavailable(_)),
            "expected ProviderUnavailable, got {error:?}"
        );
        assert!(
            error.to_string().contains("ANTHROPIC_API_KEY"),
            "error message must name ANTHROPIC_API_KEY; got: {error}"
        );
    }

    /// Verifies that selecting `EXTRACT_SESSION_PROVIDER=claude-code` (the CLI subscription
    /// path) and attempting an extraction when the CLI binary is absent surfaces loudly as
    /// `ExtractionError::ProviderUnavailable` — no silent fallback.
    #[tokio::test]
    async fn env_route_claude_code_with_absent_cli_fails_loudly_at_extraction() {
        use domain::{DomainId, TranscriptEntry, TranscriptSkillExtractionService};
        use infrastructure::{ClaudeCodeExtractionConfig, ClaudeCodeExtractor};

        let provider =
            ExtractionProvider::from_str("claude-code").expect("'claude-code' must parse");
        assert_eq!(
            provider,
            ExtractionProvider::ClaudeCode,
            "'claude-code' must route to the CLI provider"
        );

        // Construction succeeds even without a reachable CLI binary (deliberate design:
        // the CLI is probed at extraction time, not at construction).
        let config = ClaudeCodeExtractionConfig {
            cli_path: "/nonexistent-claude-binary-path-for-test".to_owned(),
            ..ClaudeCodeExtractionConfig::default()
        };
        let extractor = ClaudeCodeExtractor::new(config)
            .expect("ClaudeCodeExtractor construction must succeed even with absent CLI");

        let transcript = domain::SessionTranscript {
            session_id: DomainId::new_unchecked("cli-absent-test"),
            entries: vec![TranscriptEntry {
                speaker: "user".to_owned(),
                content: "test content".to_owned(),
            }],
        };

        let error = extractor
            .extract(&transcript)
            .await
            .expect_err("extraction with absent CLI must fail loudly");
        assert!(
            matches!(error, domain::ExtractionError::ProviderUnavailable(_)),
            "expected ProviderUnavailable for absent CLI, got {error:?}"
        );
    }

    fn sandbox_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lib-test-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&dir).expect("sandbox dir should be creatable");
        dir
    }

    fn inline_transcript() -> String {
        r#"{"type":"message","message":{"role":"user","content":"setup rust io helpers"}}"#
            .to_owned()
    }

    fn request_for(session_id: &str) -> ExtractSessionRequest {
        ExtractSessionRequest {
            transcript_ref: "ignored".to_owned(),
            transcript_inline: Some(inline_transcript()),
            session_id: session_id.to_owned(),
            repo_path: None,
        }
    }

    #[derive(Clone)]
    struct StaticExtractor {
        delay: Duration,
        fail_until: Arc<AtomicU32>,
    }

    impl StaticExtractor {
        fn ok(delay: Duration) -> Self {
            Self {
                delay,
                fail_until: Arc::new(AtomicU32::new(0)),
            }
        }

        fn failing_for(attempts: u32) -> Self {
            Self {
                delay: Duration::ZERO,
                fail_until: Arc::new(AtomicU32::new(attempts)),
            }
        }
    }

    #[async_trait]
    impl TranscriptSkillExtractionService for StaticExtractor {
        async fn extract(
            &self,
            _transcript: &SessionTranscript,
        ) -> Result<ExtractionResult, ExtractionError> {
            if self.fail_until.load(Ordering::SeqCst) > 0 {
                self.fail_until.fetch_sub(1, Ordering::SeqCst);
                return Err(ExtractionError::ProviderUnavailable("transient".to_owned()));
            }
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            Ok(ExtractionResult {
                source_session_id: DomainId::new_unchecked("lib-test"),
                provider: "ollama".to_owned(),
                candidates: vec![ExtractedSkillCandidate {
                    name: "lib-skill".to_owned(),
                    description: "desc".to_owned(),
                    tags: vec![],
                    procedures: vec!["step".to_owned()],
                    conventions: vec![],
                    assets: vec![],
                    confidence: 0.9,
                }],
            })
        }
    }

    fn build_no_pool_extractor(
        sandbox: &std::path::Path,
        transcript_root: &std::path::Path,
        extractor_impl: Arc<dyn TranscriptSkillExtractionService>,
        pool_config: ExtractionWorkerPoolConfig,
        worker_pool: Option<ExtractionWorkerPool>,
    ) -> SessionExtractor {
        let retry_policy = pool_config.retry_policy.clone();
        let job_timeout = pool_config.timeout;
        SessionExtractor {
            provider: ExtractionProvider::Ollama,
            extractor: extractor_impl,
            transcript_loader: TranscriptLoader::new(transcript_root.to_path_buf())
                .expect("loader"),
            draft_writer: PendingDraftWriter::new(vec![sandbox.to_path_buf()]),
            lifecycle_events: ExtractionLifecycleEvents::default(),
            event_publisher: Arc::new(NoopExtractionEventPublisher),
            worker_pool,
            retry_policy,
            job_timeout,
        }
    }

    #[tokio::test]
    async fn no_pool_path_emits_completed_event() {
        let sandbox = sandbox_dir("no-pool-ok");
        let transcript_root = sandbox_dir("no-pool-ok-tx");
        let extractor = build_no_pool_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(StaticExtractor::ok(Duration::ZERO)),
            ExtractionWorkerPoolConfig::default(),
            None,
        );

        let response = extractor.enqueue(request_for("no-pool-ok")).await;
        assert_eq!(response.status, "processing");

        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(25)).await;
            if extractor
                .lifecycle_events()
                .iter()
                .any(|event| event.event_type == "extraction.completed")
            {
                return;
            }
        }
        panic!("no-pool path did not emit extraction.completed");
    }

    #[tokio::test]
    async fn no_pool_path_has_timeout_arm() {
        // The no-pool spawn fallback must own a timeout arm: a slow extraction
        // must surface extraction.failed ("extraction timed out"), never stall
        // silently.
        let sandbox = sandbox_dir("no-pool-timeout");
        let transcript_root = sandbox_dir("no-pool-timeout-tx");
        let extractor = build_no_pool_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(StaticExtractor::ok(Duration::from_secs(10))),
            ExtractionWorkerPoolConfig::default().with_timeout(Duration::from_millis(100)),
            None,
        );

        let response = extractor.enqueue(request_for("no-pool-timeout")).await;
        assert_eq!(response.status, "processing");

        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let timed_out = extractor.lifecycle_events().iter().any(|event| {
                event.event_type == "extraction.failed"
                    && event
                        .payload
                        .as_object()
                        .and_then(|payload| payload.get("error"))
                        .and_then(|value| value.as_str())
                        == Some("extraction timed out")
            });
            if timed_out {
                return;
            }
        }
        panic!("no-pool path did not emit a timeout extraction.failed event");
    }

    #[tokio::test]
    async fn retry_policy_is_config_sourced_and_runs() {
        // The unified config-sourced retry must actually re-run the extraction
        // call: an extractor that fails twice then succeeds must complete when
        // max_attempts >= 3.
        let sandbox = sandbox_dir("retry");
        let transcript_root = sandbox_dir("retry-tx");
        let extractor = build_no_pool_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(StaticExtractor::failing_for(2)),
            ExtractionWorkerPoolConfig::default(),
            None,
        );

        let paths = extractor
            .extract_blocking(&request_for("retry"))
            .await
            .expect("retry should recover after two transient failures");
        assert!(!paths.is_empty());
    }

    #[tokio::test]
    async fn completed_event_draft_paths_are_scope_relative() {
        // Security P1: the extraction.completed payload must never leak absolute
        // host paths. draft_paths must be scope-relative.
        let sandbox = sandbox_dir("scope-relative");
        let transcript_root = sandbox_dir("scope-relative-tx");
        let extractor = build_no_pool_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(StaticExtractor::ok(Duration::ZERO)),
            ExtractionWorkerPoolConfig::default(),
            None,
        );

        extractor
            .extract_blocking(&request_for("scope-relative"))
            .await
            .expect("extraction should succeed");

        let events = extractor.lifecycle_events();
        let completed = events
            .iter()
            .find(|event| event.event_type == "extraction.completed")
            .expect("completed event must be present");
        let draft_paths = completed
            .payload
            .get("draft_paths")
            .and_then(|value| value.as_array())
            .expect("draft_paths array must be present");
        assert!(!draft_paths.is_empty(), "at least one draft path expected");
        for path in draft_paths {
            let path = path.as_str().expect("draft path is a string");
            assert!(
                !path.starts_with('/'),
                "draft path must not be an absolute unix path: {path}"
            );
            assert!(
                !path.contains(":\\"),
                "draft path must not be an absolute windows path: {path}"
            );
            assert!(
                !path.starts_with(sandbox.to_str().unwrap()),
                "draft path must not contain the absolute host scope root: {path}"
            );
        }
    }

    /// Proves that `SessionExtractionError::Extraction(ProviderUnavailable)` maps to the
    /// distinct `"provider_unavailable"` reason code, not the coarse `"extraction_failed"`.
    ///
    /// An agent must be able to distinguish CLI misconfiguration (alert: fix config)
    /// from a generic extraction failure (retry: transient), so the two codes must be
    /// separate. This test locks the string so a rename surfaces immediately.
    #[test]
    fn provider_unavailable_extraction_error_maps_to_distinct_reason_code() {
        let error = SessionExtractionError::Extraction(ExtractionError::ProviderUnavailable(
            "cli binary not found".to_owned(),
        ));
        assert_eq!(
            error.reason_code(),
            "provider_unavailable",
            "ProviderUnavailable must produce reason_code 'provider_unavailable', not 'extraction_failed'"
        );
    }

    /// Proves that non-ProviderUnavailable extraction errors still map to the
    /// catch-all `"extraction_failed"` reason code (regression guard).
    #[test]
    fn non_provider_unavailable_extraction_errors_map_to_extraction_failed() {
        let error = SessionExtractionError::Extraction(ExtractionError::Unexpected(
            "some transient error".to_owned(),
        ));
        assert_eq!(
            error.reason_code(),
            "extraction_failed",
            "Unexpected extraction errors must still map to 'extraction_failed'"
        );
    }

    /// Proves that the `extraction.failed` event payload includes a `reason_code`
    /// field so agents can programmatically branch on failure category without
    /// string-parsing the `error` field.
    #[tokio::test]
    async fn extraction_failed_event_includes_reason_code_in_payload() {
        let sandbox = sandbox_dir("failed-event-reason");
        let transcript_root = sandbox_dir("failed-event-reason-tx");

        // Build a provider-unavailable extractor so the job fails with ProviderUnavailable.
        struct ProviderUnavailableExtractor;
        #[async_trait]
        impl TranscriptSkillExtractionService for ProviderUnavailableExtractor {
            async fn extract(
                &self,
                _transcript: &SessionTranscript,
            ) -> Result<ExtractionResult, ExtractionError> {
                Err(ExtractionError::ProviderUnavailable(
                    "cli not found in test".to_owned(),
                ))
            }
        }

        let pool_config = ExtractionWorkerPoolConfig::default();
        let extractor = build_no_pool_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(ProviderUnavailableExtractor),
            pool_config,
            None,
        );

        // Use extract_blocking which publishes the terminal event synchronously.
        let _ = extractor
            .extract_blocking(&request_for("failed-event"))
            .await;

        let events = extractor.lifecycle_events();
        let failed = events
            .iter()
            .find(|event| event.event_type == "extraction.failed")
            .expect("extraction.failed event must be emitted on ProviderUnavailable");

        let payload = failed
            .payload
            .as_object()
            .expect("payload must be a JSON object");
        let reason_code = payload
            .get("reason_code")
            .and_then(|v| v.as_str())
            .expect("extraction.failed payload must include a 'reason_code' field");

        assert_eq!(
            reason_code, "provider_unavailable",
            "reason_code in extraction.failed event must be 'provider_unavailable' for ProviderUnavailable errors"
        );
    }
}
