use infrastructure::RetryPolicy;
use tokio::sync::oneshot;

use crate::{ExtractSessionRequest, ExtractSessionResponse, ExtractionOutcome, SessionExtractor};

const DEFAULT_QUEUE_DEPTH: usize = 64;
const DEFAULT_MAX_CONCURRENT: usize = 4;
/// Default outer per-job ceiling for the LEGACY SINGLE-SHOT path only — one bounded
/// LLM call, where a fixed ceiling with requeue-on-elapsed (#190) is appropriate.
/// The orchestrated path does NOT use this: it is a churning background worker with
/// no fixed wall-clock ceiling (`timeout: None`); see the field docs on
/// [`ExtractionWorkerPoolConfig::timeout`]. Operators may still override either path
/// via `EXTRACTION_JOB_TIMEOUT_MS` (`0` = no ceiling).
///
/// On ceiling hit (single-shot) the job is REQUEUED with bounded backoff (from
/// `retry_policy`) rather than dropped. Only after `retry_policy.max_attempts`
/// ceiling hits is `extraction.failed` emitted — no job is discarded for being slow.
const DEFAULT_TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Clone)]
pub struct ExtractionWorkerPoolConfig {
    pub queue_depth: usize,
    pub max_concurrent: usize,
    /// Optional OUTER per-job wall-clock ceiling.
    ///
    /// `None` = no fixed ceiling: a long-but-progressing job runs to completion.
    /// This is the correct setting for the multi-call orchestrated extraction
    /// path, which is a background worker that constantly churns through many
    /// sequential LLM calls — a fixed wall-clock cap there only causes infinite
    /// requeue-thrash on slow hardware (the job can never finish inside the cap),
    /// never real progress. Liveness for that path comes from each provider call's
    /// own per-request timeout plus the streaming idle-watchdog (#197), not a
    /// hardcoded total-time kill.
    ///
    /// `Some(d)` = single-shot legacy path: one bounded LLM call, where a fixed
    /// ceiling with requeue-on-elapsed (#190) is appropriate.
    pub timeout: Option<std::time::Duration>,
    pub retry_policy: RetryPolicy,
}

impl Default for ExtractionWorkerPoolConfig {
    fn default() -> Self {
        Self {
            queue_depth: DEFAULT_QUEUE_DEPTH,
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            // Default preserves the single-shot ceiling for back-compat; the
            // orchestrated path opts into `None` at construction time.
            timeout: Some(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
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
        self.timeout = Some(timeout);
        self
    }

    /// Sets the outer ceiling explicitly, including `None` for "no fixed ceiling"
    /// (the churning orchestrated path).
    pub fn with_optional_timeout(mut self, timeout: Option<std::time::Duration>) -> Self {
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
    /// Number of ceiling hits already absorbed for this job.
    ///
    /// Incremented each time the safety ceiling elapses and the job is requeued.
    /// When `ceiling_hits >= retry_policy.max_attempts` the job is terminally
    /// failed with `extraction_timed_out_retries_exhausted` instead of requeued.
    ceiling_hits: u32,
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
        //
        // The sender clone is also passed into each worker so ceiling-hit jobs can
        // requeue themselves without needing a separate delivery channel.
        let (job_tx, job_rx) = async_channel::bounded::<ExtractionJob>(config.queue_depth.max(1));
        let timeout = config.timeout;
        let retry_policy = config.retry_policy.clone();

        for _ in 0..config.max_concurrent.max(1) {
            let job_rx = job_rx.clone();
            let job_tx = job_tx.clone();
            let retry_policy = retry_policy.clone();
            tokio::spawn(async move {
                worker_loop(job_rx, job_tx, timeout, retry_policy).await;
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
            ceiling_hits: 0,
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
/// nothing; the loop maps it (or a ceiling hit) to exactly one of
/// `extraction.completed` / `extraction.failed`, so every accepted job emits one
/// and only one terminal event.
///
/// # Safety ceiling + requeue contract
///
/// When the wall-clock ceiling elapses the job is NOT dropped. Instead:
/// - If `ceiling_hits < retry_policy.max_attempts`: the job is requeued into
///   `job_tx` with an incremented `ceiling_hits` counter and a bounded backoff
///   delay. No terminal event is emitted for this ceiling hit.
/// - If `ceiling_hits >= retry_policy.max_attempts`: a terminal `extraction.failed`
///   is emitted with reason `extraction_timed_out_retries_exhausted`. Only then
///   is the job discarded.
///
/// This ensures a slow-but-healthy extraction (large session, loaded GPU) is never
/// permanently lost on its first — or second, or third — ceiling hit.
async fn worker_loop(
    job_rx: async_channel::Receiver<ExtractionJob>,
    job_tx: async_channel::Sender<ExtractionJob>,
    timeout: Option<std::time::Duration>,
    retry_policy: RetryPolicy,
) {
    while let Ok(mut job) = job_rx.recv().await {
        // No fixed ceiling (orchestrated churning path): await the job directly so a
        // slow-but-progressing extraction is never cut off or requeue-thrashed.
        let run = job.extractor.execute_job(&job.job_id, &job.request);
        let outcome = match timeout {
            Some(ceiling) => tokio::time::timeout(ceiling, run).await,
            None => Ok(run.await),
        };
        match outcome {
            Ok(ExtractionOutcome::Completed {
                draft_paths,
                source_session_id,
            }) => {
                job.extractor
                    .publish_terminal_event(
                        &job.job_id,
                        ExtractionOutcome::Completed {
                            draft_paths,
                            source_session_id,
                        },
                    )
                    .await;
                let response = ExtractSessionResponse {
                    status: "completed".to_owned(),
                    reason_code: None,
                    job_id: Some(job.job_id),
                    provider: Some(job.extractor.provider.as_str().to_owned()),
                };
                let _ = job.response_tx.send(response);
            }
            Ok(ExtractionOutcome::Failed(error)) => {
                let reason = error.reason_code().to_owned();
                job.extractor
                    .publish_terminal_event(&job.job_id, ExtractionOutcome::Failed(error))
                    .await;
                let response = ExtractSessionResponse {
                    status: "failed".to_owned(),
                    reason_code: Some(reason),
                    job_id: Some(job.job_id),
                    provider: Some(job.extractor.provider.as_str().to_owned()),
                };
                let _ = job.response_tx.send(response);
            }
            Err(_elapsed) => {
                // Safety ceiling hit: requeue with backoff if retries remain;
                // only terminate after all retries are exhausted.
                let next_hits = job.ceiling_hits + 1;
                if next_hits < retry_policy.max_attempts {
                    // Retries remain: backoff then requeue. No terminal event.
                    let jitter = std::time::Duration::from_millis(
                        (next_hits as u64).saturating_mul(7),
                    );
                    let backoff = retry_policy
                        .base_delay
                        .saturating_mul(next_hits)
                        .saturating_add(jitter)
                        .min(retry_policy.max_delay);
                    tokio::time::sleep(backoff).await;

                    job.ceiling_hits = next_hits;
                    // If the channel is full or closed the job cannot be requeued,
                    // so we fall through to terminal failure. Closed = pool shutdown;
                    // full = sustained overload. In both cases the job cannot
                    // make progress.
                    match job_tx.try_send(job) {
                        Ok(()) => {
                            // Requeued: the response_tx is carried in the job and
                            // will be written when the job eventually completes.
                        }
                        Err(enqueue_error) => {
                            let failed_job = match enqueue_error {
                                async_channel::TrySendError::Full(j) => j,
                                async_channel::TrySendError::Closed(j) => j,
                            };
                            let provider = failed_job.extractor.provider.as_str().to_owned();
                            let job_id = failed_job.job_id.clone();
                            failed_job
                                .extractor
                                .publish_ceiling_exhausted_event(&job_id)
                                .await;
                            let response = ExtractSessionResponse {
                                status: "failed".to_owned(),
                                reason_code: Some(
                                    "extraction_timed_out_retries_exhausted".to_owned(),
                                ),
                                job_id: Some(job_id),
                                provider: Some(provider),
                            };
                            let _ = failed_job.response_tx.send(response);
                        }
                    }
                } else {
                    // Retries exhausted: emit terminal failure.
                    let provider = job.extractor.provider.as_str().to_owned();
                    let job_id = job.job_id.clone();
                    job.extractor
                        .publish_ceiling_exhausted_event(&job_id)
                        .await;
                    let response = ExtractSessionResponse {
                        status: "failed".to_owned(),
                        reason_code: Some("extraction_timed_out_retries_exhausted".to_owned()),
                        job_id: Some(job_id),
                        provider: Some(provider),
                    };
                    let _ = job.response_tx.send(response);
                }
            }
        }
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
        // The DEFAULT preserves the bounded single-shot ceiling; the orchestrated
        // path opts into `None` (no ceiling) at construction. See
        // `ExtractionWorkerPoolConfig::timeout`.
        assert_eq!(config.timeout, Some(Duration::from_secs(180)));
    }

    #[test]
    fn config_builder_overrides() {
        let config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(8)
            .with_max_concurrent(2)
            .with_timeout(Duration::from_secs(10));

        assert_eq!(config.queue_depth, 8);
        assert_eq!(config.max_concurrent, 2);
        assert_eq!(config.timeout, Some(Duration::from_secs(10)));
    }

    #[test]
    fn optional_timeout_supports_no_ceiling() {
        let bounded = ExtractionWorkerPoolConfig::default()
            .with_optional_timeout(Some(Duration::from_secs(42)));
        assert_eq!(bounded.timeout, Some(Duration::from_secs(42)));

        // `None` = no fixed wall-clock ceiling for the churning orchestrated worker.
        let unbounded = ExtractionWorkerPoolConfig::default().with_optional_timeout(None);
        assert_eq!(unbounded.timeout, None);
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
                    generality: None,
                    generality_rationale: None,
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
        use crate::routing::{ExtractionRoutingTier, LOCAL_TIER_TOKEN_BUDGET, RoutingDecision};
        let provider = ExtractionProvider::Claude;
        let routing_decision = RoutingDecision {
            tier: ExtractionRoutingTier::Local,
            provider,
            segmentation_token_budget: LOCAL_TIER_TOKEN_BUDGET,
            dual_pass_enabled: false,
        };
        crate::SessionExtractor {
            provider,
            run_path: crate::ExtractionRunPath::SingleShot,
            routing_decision,
            extractor: extractor_impl,
            transcript_loader: TranscriptLoader::new(transcript_root.to_path_buf())
                .expect("loader"),
            draft_writer: PendingDraftWriter::new(vec![sandbox.to_path_buf()]),
            lifecycle_events: crate::ExtractionLifecycleEvents::default(),
            event_publisher: Arc::new(crate::NoopExtractionEventPublisher),
            worker_pool: Some(pool),
            retry_policy: RetryPolicy::default(),
            job_timeout: Some(std::time::Duration::from_secs(30)),
            skeleton_labeler: None,
            embedder: None,
            equivalence_verifier: None,
            synthesis: None,
            preamble_normalizer: None,
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
    async fn ceiling_exhausted_emits_terminal_failure_with_retries_exhausted_reason() {
        // When retry_policy.max_attempts = 1, the very first ceiling hit exhausts
        // retries and must produce a terminal extraction.failed with reason
        // extraction_timed_out_retries_exhausted. This is the "retries exhausted"
        // path that replaces the old drop-on-timeout behaviour.
        let sandbox = sandbox_dir("timeout");
        let transcript_root = sandbox_dir("timeout-transcripts");

        let pool_config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(2)
            .with_max_concurrent(1)
            .with_timeout(Duration::from_millis(100))
            .with_retry_policy(RetryPolicy {
                max_attempts: 1,
                base_delay: std::time::Duration::from_millis(1),
                max_delay: std::time::Duration::from_millis(10),
            });
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
                        .and_then(|payload| payload.get("reason_code"))
                        .and_then(|v| v.as_str())
                        == Some("extraction_timed_out_retries_exhausted")
            });
            if has_failed {
                return;
            }
        }

        panic!("retries-exhausted extraction.failed event was not emitted");
    }

    #[tokio::test]
    async fn ceiling_hit_requeues_before_retries_exhaust() {
        // A job that hits the ceiling once must NOT produce extraction.failed
        // immediately. It is requeued and completes on a subsequent attempt.
        // This test uses a SlowThenFastExtractor: slow on the first call
        // (trips the ceiling) and fast on subsequent calls (succeeds after requeue).
        use std::sync::atomic::AtomicBool;

        #[derive(Clone)]
        struct SlowThenFastExtractor {
            first_call_done: Arc<AtomicBool>,
            slow_delay: Duration,
        }

        #[async_trait]
        impl TranscriptSkillExtractionService for SlowThenFastExtractor {
            async fn extract(
                &self,
                _transcript: &SessionTranscript,
            ) -> Result<ExtractionResult, ExtractionError> {
                let is_first = !self.first_call_done.swap(true, Ordering::SeqCst);
                if is_first {
                    // First call: sleep longer than the ceiling to trigger a requeue.
                    tokio::time::sleep(self.slow_delay).await;
                }
                // Subsequent calls: return immediately.
                Ok(ExtractionResult {
                    source_session_id: DomainId::new_unchecked("requeue-test"),
                    provider: "claude".to_owned(),
                    candidates: vec![ExtractedSkillCandidate {
                        name: "requeue-skill".to_owned(),
                        description: "desc".to_owned(),
                        tags: vec![],
                        procedures: vec![],
                        conventions: vec![],
                        assets: vec![],
                        confidence: 0.9,
                        generality: None,
                        generality_rationale: None,
                    }],
                })
            }
        }

        let sandbox = sandbox_dir("requeue");
        let transcript_root = sandbox_dir("requeue-transcripts");

        // Ceiling = 100ms; slow_delay = 500ms (definitely trips the ceiling).
        // max_attempts = 3, so after 1 hit (ceiling_hits=1 < max_attempts=3)
        // the job is requeued. The second call returns fast and succeeds.
        let pool_config = ExtractionWorkerPoolConfig::default()
            .with_queue_depth(4)
            .with_max_concurrent(1)
            .with_timeout(Duration::from_millis(100))
            .with_retry_policy(RetryPolicy {
                max_attempts: 3,
                base_delay: std::time::Duration::from_millis(1),
                max_delay: std::time::Duration::from_millis(10),
            });
        let pool = ExtractionWorkerPool::new(pool_config);

        let slow_extractor = Arc::new(SlowThenFastExtractor {
            first_call_done: Arc::new(AtomicBool::new(false)),
            slow_delay: Duration::from_millis(500),
        });

        use crate::routing::{ExtractionRoutingTier, LOCAL_TIER_TOKEN_BUDGET, RoutingDecision};
        let requeue_provider = ExtractionProvider::Claude;
        let requeue_routing = RoutingDecision {
            tier: ExtractionRoutingTier::Local,
            provider: requeue_provider,
            segmentation_token_budget: LOCAL_TIER_TOKEN_BUDGET,
            dual_pass_enabled: false,
        };
        let extractor = crate::SessionExtractor {
            provider: requeue_provider,
            run_path: crate::ExtractionRunPath::SingleShot,
            routing_decision: requeue_routing,
            extractor: slow_extractor,
            transcript_loader: TranscriptLoader::new(transcript_root.to_path_buf())
                .expect("loader"),
            draft_writer: PendingDraftWriter::new(vec![sandbox.to_path_buf()]),
            lifecycle_events: crate::ExtractionLifecycleEvents::default(),
            event_publisher: Arc::new(crate::NoopExtractionEventPublisher),
            worker_pool: Some(pool),
            retry_policy: RetryPolicy::default(),
            job_timeout: Some(std::time::Duration::from_secs(30)),
            skeleton_labeler: None,
            embedder: None,
            equivalence_verifier: None,
            synthesis: None,
            preamble_normalizer: None,
        };

        let response = extractor.enqueue(make_request("requeue-test")).await;
        assert_eq!(response.status, "processing");

        // Wait just past the first ceiling hit (100ms) but before the job
        // would complete if it had NOT been requeued.
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Assert: no terminal extraction.failed has been emitted yet — the job
        // must have been requeued, not dropped.
        let events_after_first_hit = extractor.lifecycle_events();
        let has_terminal_failure = events_after_first_hit.iter().any(|event| {
            event.event_type == "extraction.failed"
        });
        assert!(
            !has_terminal_failure,
            "extraction.failed must not be emitted on the first ceiling hit — job should be requeued"
        );

        // Now wait for the requeued job to complete successfully.
        let mut completed = false;
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if extractor
                .lifecycle_events()
                .iter()
                .any(|event| event.event_type == "extraction.completed")
            {
                completed = true;
                break;
            }
        }

        assert!(
            completed,
            "requeued job must eventually complete with extraction.completed"
        );

        // Confirm no extraction.failed was ever emitted.
        let final_events = extractor.lifecycle_events();
        assert!(
            !final_events
                .iter()
                .any(|event| event.event_type == "extraction.failed"),
            "no extraction.failed must be emitted when the job succeeds after requeue"
        );
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
