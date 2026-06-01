use std::sync::Arc;

use infrastructure::RetryPolicy;
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::{ExtractSessionRequest, ExtractSessionResponse, SessionExtractor};

const DEFAULT_QUEUE_DEPTH: usize = 64;
const DEFAULT_MAX_CONCURRENT: usize = 4;
const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct ExtractionWorkerPoolConfig {
    pub queue_depth: usize,
    pub max_concurrent: usize,
    pub timeout: std::time::Duration,
    pub retry_policy: RetryPolicy,
}

impl Default for ExtractionWorkerPoolConfig {
    fn default() -> Self {
        Self {
            queue_depth: DEFAULT_QUEUE_DEPTH,
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            retry_policy: RetryPolicy::default(),
        }
    }
}

impl ExtractionWorkerPoolConfig {
    pub fn with_queue_depth(mut self, queue_depth: usize) -> Self {
        self.queue_depth = queue_depth;
        self
    }

    pub fn with_max_concurrent(mut self, max_concurrent: usize) -> Self {
        self.max_concurrent = max_concurrent;
        self
    }

    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }
}

struct ExtractionJob {
    extractor: SessionExtractor,
    job_id: String,
    request: ExtractSessionRequest,
    response_tx: oneshot::Sender<ExtractSessionResponse>,
}

#[derive(Clone)]
pub struct ExtractionWorkerPool {
    job_tx: mpsc::Sender<ExtractionJob>,
}

impl ExtractionWorkerPool {
    pub fn new(config: ExtractionWorkerPoolConfig) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<ExtractionJob>(config.queue_depth.max(1));
        let rx = Arc::new(tokio::sync::Mutex::new(job_rx));
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent.max(1)));
        let timeout = config.timeout;
        let retry_policy = config.retry_policy;

        for _ in 0..config.max_concurrent.max(1) {
            let rx = Arc::clone(&rx);
            let semaphore = Arc::clone(&semaphore);
            let timeout = timeout;
            let retry_policy = retry_policy.clone();
            tokio::spawn(async move {
                worker_loop(rx, semaphore, timeout, retry_policy).await;
            });
        }

        Self { job_tx }
    }

    pub fn submit(
        &self,
        extractor: SessionExtractor,
        job_id: String,
        request: ExtractSessionRequest,
    ) -> Result<oneshot::Receiver<ExtractSessionResponse>, ExtractSessionResponse> {
        let provider = extractor.provider.as_str().to_owned();
        let (response_tx, response_rx) = oneshot::channel();
        let job = ExtractionJob {
            extractor,
            job_id,
            request,
            response_tx,
        };

        match self.job_tx.try_send(job) {
            Ok(()) => Ok(response_rx),
            Err(mpsc::error::TrySendError::Full(_)) => Err(ExtractSessionResponse {
                status: "rejected".to_owned(),
                reason_code: Some("extraction_queue_full".to_owned()),
                job_id: None,
                provider: Some(provider),
            }),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(ExtractSessionResponse {
                status: "rejected".to_owned(),
                reason_code: Some("extraction_pool_shutdown".to_owned()),
                job_id: None,
                provider: Some(provider),
            }),
        }
    }
}

async fn worker_loop(
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<ExtractionJob>>>,
    semaphore: Arc<Semaphore>,
    timeout: std::time::Duration,
    _retry_policy: RetryPolicy,
) {
    loop {
        let job = {
            let mut rx = rx.lock().await;
            rx.recv().await
        };

        let ExtractionJob {
            extractor,
            job_id,
            request,
            response_tx,
        } = match job {
            Some(job) => job,
            None => return,
        };

        let _permit = semaphore.acquire().await;

        let result = tokio::time::timeout(timeout, extractor.execute_job(&job_id, &request)).await;

        let response = match result {
            Ok(Ok(_draft_paths)) => ExtractSessionResponse {
                status: "completed".to_owned(),
                reason_code: None,
                job_id: Some(job_id),
                provider: Some(extractor.provider.as_str().to_owned()),
            },
            Ok(Err(error)) => {
                let reason = error.reason_code().to_owned();
                let _ = extractor
                    .publish_lifecycle_event(infrastructure::EventEnvelope::new(
                        "extraction.failed",
                        format!("extraction.failed:{job_id}"),
                        serde_json::json!({
                            "job_id": job_id.as_str(),
                            "provider": extractor.provider.as_str(),
                            "error": error.to_string(),
                        }),
                    ))
                    .await;
                ExtractSessionResponse {
                    status: "failed".to_owned(),
                    reason_code: Some(reason),
                    job_id: Some(job_id),
                    provider: Some(extractor.provider.as_str().to_owned()),
                }
            }
            Err(_elapsed) => {
                let _ = extractor
                    .publish_lifecycle_event(infrastructure::EventEnvelope::new(
                        "extraction.failed",
                        format!("extraction.failed:{job_id}"),
                        serde_json::json!({
                            "job_id": job_id.as_str(),
                            "provider": extractor.provider.as_str(),
                            "error": "extraction timed out",
                        }),
                    ))
                    .await;
                ExtractSessionResponse {
                    status: "timeout".to_owned(),
                    reason_code: Some("extraction_timed_out".to_owned()),
                    job_id: Some(job_id),
                    provider: Some(extractor.provider.as_str().to_owned()),
                }
            }
        };

        let _ = response_tx.send(response);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, atomic::{AtomicU32, Ordering}},
        time::Duration,
    };

    use async_trait::async_trait;
    use domain::{
        DomainId, ExtractedSkillCandidate, ExtractionError, ExtractionResult, SessionTranscript,
        TranscriptSkillExtractionService,
    };

    use super::*;
    use crate::{
        ExtractionProvider, transcripts::TranscriptLoader, writer::PendingDraftWriter,
    };

    #[test]
    fn config_defaults_match_constants() {
        let config = ExtractionWorkerPoolConfig::default();
        assert_eq!(config.queue_depth, 64);
        assert_eq!(config.max_concurrent, 4);
        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[test]
    fn config_builder_overrides() {
        let config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(8)
            .with_max_concurrent(2)
            .with_timeout(Duration::from_secs(10));

        assert_eq!(config.queue_depth, 8);
        assert_eq!(config.max_concurrent, 2);
        assert_eq!(config.timeout, Duration::from_secs(10));
    }

    fn sample_inline_transcript() -> String {
        r#"{"type":"message","message":{"role":"user","content":"setup rust io helpers"}}
{"type":"message","message":{"role":"assistant","content":"use Result and share them"}}"#
            .to_owned()
    }

    fn sandbox_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "worker-pool-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("sandbox dir should be creatable");
        dir
    }

    #[derive(Clone)]
    struct CountingExtractor {
        call_count: Arc<AtomicU32>,
        delay: Duration,
    }

    impl CountingExtractor {
        fn new(delay: Duration) -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                delay,
            }
        }
    }

    #[async_trait]
    impl TranscriptSkillExtractionService for CountingExtractor {
        async fn extract(
            &self,
            _transcript: &SessionTranscript,
        ) -> Result<ExtractionResult, ExtractionError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            Ok(ExtractionResult {
                source_session_id: DomainId::new_unchecked("pool-test"),
                provider: "claude".to_owned(),
                candidates: vec![ExtractedSkillCandidate {
                    name: "pool-skill".to_owned(),
                    description: "desc".to_owned(),
                    tags: vec![],
                    procedures: vec![],
                    conventions: vec![],
                    assets: vec![],
                    confidence: 0.9,
                }],
            })
        }
    }

    fn build_extractor(
        sandbox: &std::path::Path,
        transcript_root: &std::path::Path,
        extractor_impl: Arc<CountingExtractor>,
        pool: ExtractionWorkerPool,
    ) -> crate::SessionExtractor {
        crate::SessionExtractor {
            provider: ExtractionProvider::Claude,
            extractor: extractor_impl,
            transcript_loader: TranscriptLoader::new(transcript_root.to_path_buf())
                .expect("loader"),
            draft_writer: PendingDraftWriter::new(vec![sandbox.to_path_buf()]),
            lifecycle_events: crate::ExtractionLifecycleEvents::default(),
            event_publisher: Arc::new(crate::NoopExtractionEventPublisher),
            worker_pool: Some(pool),
        }
    }

    fn make_request(session_id: &str) -> ExtractSessionRequest {
        ExtractSessionRequest {
            transcript_ref: "ignored".to_owned(),
            transcript_inline: Some(sample_inline_transcript()),
            session_id: session_id.to_owned(),
            repo_path: None,
        }
    }

    #[tokio::test]
    async fn queue_full_rejects_with_reason_code() {
        let sandbox = sandbox_dir("queue-full");
        let transcript_root = sandbox_dir("queue-full-transcripts");

        let pool_config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(1)
            .with_max_concurrent(1);
        let pool = ExtractionWorkerPool::new(pool_config);

        let extractor = build_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(CountingExtractor::new(Duration::from_secs(5))),
            pool,
        );

        let _first = extractor.enqueue(make_request("s1")).await;

        let second = extractor.enqueue(make_request("s2")).await;

        assert_eq!(second.status, "rejected");
        assert_eq!(second.reason_code.as_deref(), Some("extraction_queue_full"));
        assert!(second.job_id.is_none());
    }

    #[tokio::test]
    async fn timeout_produces_explicit_reason_code() {
        let sandbox = sandbox_dir("timeout");
        let transcript_root = sandbox_dir("timeout-transcripts");

        let pool_config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(2)
            .with_max_concurrent(1)
            .with_timeout(Duration::from_millis(100));
        let pool = ExtractionWorkerPool::new(pool_config);

        let extractor = build_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(CountingExtractor::new(Duration::from_secs(5))),
            pool,
        );

        let response = extractor.enqueue(make_request("timeout-test")).await;

        assert_eq!(response.status, "processing");

        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let events = extractor.lifecycle_events();
            let has_failed = events.iter().any(|event| {
                event.event_type == "extraction.failed"
                    && event
                        .payload
                        .as_object()
                        .and_then(|payload| payload.get("error"))
                        .and_then(|v| v.as_str())
                        == Some("extraction timed out")
            });
            if has_failed {
                return;
            }
        }

        panic!("timeout failed event was not emitted");
    }

    #[tokio::test]
    async fn bounded_concurrency_limits_simultaneous_extractions() {
        let sandbox = sandbox_dir("bounded-concurrency");
        let transcript_root = sandbox_dir("bounded-concurrency-transcripts");

        let max_workers = 2;
        let pool_config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(8)
            .with_max_concurrent(max_workers)
            .with_timeout(Duration::from_secs(10));
        let pool = ExtractionWorkerPool::new(pool_config);

        let counting_extractor = CountingExtractor::new(Duration::from_millis(200));
        let call_count = Arc::clone(&counting_extractor.call_count);

        let extractor = build_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(counting_extractor),
            pool,
        );

        let total_jobs = 5;
        for i in 0..total_jobs {
            let _ = extractor.enqueue(make_request(&format!("s{i}"))).await;
        }

        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let events = extractor.lifecycle_events();
            let completed = events
                .iter()
                .filter(|event| event.event_type == "extraction.completed")
                .count();
            if completed >= total_jobs as usize {
                break;
            }
        }

        let total_calls = call_count.load(Ordering::SeqCst);
        assert_eq!(total_calls, total_jobs as u32);
    }

    #[tokio::test]
    async fn enqueue_returns_rejected_on_queue_full() {
        let sandbox = sandbox_dir("enqueue-full");
        let transcript_root = sandbox_dir("enqueue-full-transcripts");

        let pool_config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(1)
            .with_max_concurrent(1);
        let pool = ExtractionWorkerPool::new(pool_config);

        let extractor = build_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(CountingExtractor::new(Duration::from_secs(10))),
            pool,
        );

        let first = extractor.enqueue(make_request("first")).await;
        assert_eq!(first.status, "processing");

        let second = extractor.enqueue(make_request("second")).await;
        assert_eq!(second.status, "rejected");
        assert_eq!(second.reason_code.as_deref(), Some("extraction_queue_full"));
    }
}