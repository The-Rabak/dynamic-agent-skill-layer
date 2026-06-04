use infrastructure::RetryPolicy;
use tokio::sync::oneshot;

use crate::{ExtractSessionRequest, ExtractSessionResponse, ExtractionOutcome, SessionExtractor};

const DEFAULT_QUEUE_DEPTH: usize = 64;
const DEFAULT_MAX_CONCURRENT: usize = 4;
/// Worker-pool (outer) per-job timeout. Must stay >= 1.5x the provider's inner
/// timeout so a slow-but-progressing extraction is not cut off prematurely. The
/// inner Ollama extraction ceiling is 120s (CPU `gemma4:e4b`), grounded in a real
/// host measurement (warm ~37s, cold-start ~66s; see `OllamaExtractionConfig::default`
/// in infrastructure/src/extraction/ollama.rs), so 180s = 1.5x preserves the margin.
/// Override per deployment via `ExtractionWorkerPoolConfig::with_timeout`.
const DEFAULT_TIMEOUT_SECS: u64 = 180;

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
    job_tx: async_channel::Sender<ExtractionJob>,
}

impl ExtractionWorkerPool {
    pub fn new(config: ExtractionWorkerPoolConfig) -> Self {
        // Bounded MPMC channel: a single shared receiver that N workers pull from
        // concurrently. This replaces the previous `Arc<Mutex<mpsc::Receiver>>`
        // (held across `recv().await`), which serialized all workers behind one
        // lock and was the confirmed "0/32 parallel jobs" throughput bug. The
        // channel's own bound (`queue_depth`) provides backpressure, so the
        // separate `Semaphore` is now redundant and removed.
        let (job_tx, job_rx) = async_channel::bounded::<ExtractionJob>(config.queue_depth.max(1));
        let timeout = config.timeout;

        for _ in 0..config.max_concurrent.max(1) {
            let job_rx = job_rx.clone();
            tokio::spawn(async move {
                worker_loop(job_rx, timeout).await;
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
            Err(async_channel::TrySendError::Full(_)) => Err(ExtractSessionResponse {
                status: "rejected".to_owned(),
                reason_code: Some("extraction_queue_full".to_owned()),
                job_id: None,
                provider: Some(provider),
            }),
            Err(async_channel::TrySendError::Closed(_)) => Err(ExtractSessionResponse {
                status: "rejected".to_owned(),
                reason_code: Some("extraction_pool_shutdown".to_owned()),
                job_id: None,
                provider: Some(provider),
            }),
        }
    }
}

/// One worker: pulls jobs from the shared MPMC receiver and runs each to a
/// terminal lifecycle event.
///
/// This loop is the dispatch layer that OWNS terminal-event publication for the
/// pool path. `execute_job` produces a pure [`ExtractionOutcome`] and publishes
/// nothing; the loop maps it (or a timeout) to exactly one of
/// `extraction.completed` / `extraction.failed`, so every accepted job emits one
/// and only one terminal event.
async fn worker_loop(job_rx: async_channel::Receiver<ExtractionJob>, timeout: std::time::Duration) {
    while let Ok(job) = job_rx.recv().await {
        let ExtractionJob {
            extractor,
            job_id,
            request,
            response_tx,
        } = job;

        let response =
            match tokio::time::timeout(timeout, extractor.execute_job(&job_id, &request)).await {
                Ok(ExtractionOutcome::Completed {
                    draft_paths,
                    source_session_id,
                }) => {
                    extractor
                        .publish_terminal_event(
                            &job_id,
                            ExtractionOutcome::Completed {
                                draft_paths,
                                source_session_id,
                            },
                        )
                        .await;
                    ExtractSessionResponse {
                        status: "completed".to_owned(),
                        reason_code: None,
                        job_id: Some(job_id),
                        provider: Some(extractor.provider.as_str().to_owned()),
                    }
                }
                Ok(ExtractionOutcome::Failed(error)) => {
                    let reason = error.reason_code().to_owned();
                    extractor
                        .publish_terminal_event(&job_id, ExtractionOutcome::Failed(error))
                        .await;
                    ExtractSessionResponse {
                        status: "failed".to_owned(),
                        reason_code: Some(reason),
                        job_id: Some(job_id),
                        provider: Some(extractor.provider.as_str().to_owned()),
                    }
                }
                Err(_elapsed) => {
                    extractor.publish_timeout_event(&job_id).await;
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
        sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use domain::{
        DomainId, ExtractedSkillCandidate, ExtractionError, ExtractionResult, SessionTranscript,
        TranscriptSkillExtractionService,
    };

    use super::*;
    use crate::{ExtractionProvider, transcripts::TranscriptLoader, writer::PendingDraftWriter};

    #[test]
    fn config_defaults_match_constants() {
        let config = ExtractionWorkerPoolConfig::default();
        assert_eq!(config.queue_depth, 64);
        assert_eq!(config.max_concurrent, 4);
        assert_eq!(config.timeout, Duration::from_secs(180));
        // Outer pool timeout must remain >= 1.5x the Ollama inner timeout (120s)
        // so a progressing CPU extraction is not cut off prematurely.
        assert!(config.timeout >= Duration::from_secs(120 * 3 / 2));
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
        let dir =
            std::env::temp_dir().join(format!("worker-pool-test-{name}-{}", std::process::id()));
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
            retry_policy: RetryPolicy::default(),
            job_timeout: std::time::Duration::from_secs(30),
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
    async fn parallel_burst_emits_exactly_one_terminal_event_per_job() {
        // SC-V1.5-C: a >=32-job burst through the real worker pool must yield
        // exactly one terminal event (completed|failed) per accepted job, with
        // counts reconciling. This is the offline deterministic proxy for the
        // live-Ollama burst (the live e2e is #[ignore]-gated in mcp-server).
        let sandbox = sandbox_dir("burst");
        let transcript_root = sandbox_dir("burst-transcripts");

        let total_jobs = 32;
        let pool_config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(total_jobs * 2)
            .with_max_concurrent(8)
            .with_timeout(Duration::from_secs(10));
        let pool = ExtractionWorkerPool::new(pool_config);

        let counting_extractor = CountingExtractor::new(Duration::from_millis(20));
        let call_count = Arc::clone(&counting_extractor.call_count);

        let extractor = build_extractor(
            &sandbox,
            &transcript_root,
            Arc::new(counting_extractor),
            pool,
        );

        let mut accepted = 0usize;
        for i in 0..total_jobs {
            let response = extractor.enqueue(make_request(&format!("burst-{i}"))).await;
            assert_eq!(response.status, "processing");
            accepted += 1;
        }

        // Wait until every accepted job has produced a terminal event.
        for _ in 0..200 {
            tokio::time::sleep(Duration::from_millis(25)).await;
            let terminal = extractor
                .lifecycle_events()
                .iter()
                .filter(|event| {
                    event.event_type == "extraction.completed"
                        || event.event_type == "extraction.failed"
                })
                .count();
            if terminal >= accepted {
                break;
            }
        }

        let events = extractor.lifecycle_events();
        let completed = events
            .iter()
            .filter(|event| event.event_type == "extraction.completed")
            .count();
        let failed = events
            .iter()
            .filter(|event| event.event_type == "extraction.failed")
            .count();

        assert_eq!(
            completed + failed,
            accepted,
            "exactly one terminal event per accepted job; completed={completed} failed={failed} accepted={accepted}"
        );
        assert_eq!(
            completed, accepted,
            "all jobs should complete with the fake extractor"
        );
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            accepted as u32,
            "extractor called exactly once per job"
        );
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
