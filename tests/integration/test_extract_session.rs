use std::{
    env,
    ffi::OsString,
    path::Path,
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use domain::{
    DomainId, ExtractedSkillCandidate, ExtractionError, ExtractionResult, SessionTranscript,
    TranscriptSkillExtractionService,
};
use infrastructure::EventEnvelope;
use mcp_server::tools::extract_session::{ExtractSessionRequest, ExtractSessionTool};
use session_extractor::{
    ExtractionEventPublisher, ExtractionProvider, SessionExtractor, transcripts::TranscriptLoader,
    writer::PendingDraftWriter,
};

static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: String) -> Self {
        let previous = env::var_os(key);
        // SAFETY: integration tests mutate process env only while holding ENV_LOCK.
        unsafe {
            env::set_var(key, value);
        }

        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: integration tests mutate process env only while holding ENV_LOCK.
        unsafe {
            if let Some(value) = &self.previous {
                env::set_var(self.key, value);
            } else {
                env::remove_var(self.key);
            }
        }
    }
}

struct RepoPathAllowlistGuard {
    _allowed_roots: EnvVarGuard,
    _lock: MutexGuard<'static, ()>,
}

fn allowlist_repo_root(root: &Path) -> RepoPathAllowlistGuard {
    let lock = ENV_LOCK.lock().expect("env lock should not be poisoned");
    RepoPathAllowlistGuard {
        _allowed_roots: EnvVarGuard::set("SKILL_GLOBAL_ALLOWED_ROOTS", root.display().to_string()),
        _lock: lock,
    }
}

fn fresh_sandbox() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("extract-session-test-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

fn sample_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/sample-transcript.jsonl")
}

#[derive(Clone)]
struct SuccessfulExtractor;

#[async_trait]
impl TranscriptSkillExtractionService for SuccessfulExtractor {
    async fn extract(
        &self,
        transcript: &SessionTranscript,
    ) -> Result<ExtractionResult, ExtractionError> {
        Ok(ExtractionResult {
            source_session_id: transcript.session_id.clone(),
            provider: "claude".to_owned(),
            candidates: vec![ExtractedSkillCandidate {
                name: "Rust File IO Setup".to_owned(),
                description: "Reusable setup for safe file reading/writing in Rust.".to_owned(),
                tags: vec!["rust".to_owned(), "io".to_owned()],
                procedures: vec!["Use explicit Result returns for IO operations.".to_owned()],
                conventions: vec![
                    "Avoid panic-based file handling in reusable helpers.".to_owned(),
                ],
                assets: vec!["docs/rust-file-io.md".to_owned()],
                confidence: 0.92,
                generality: None,
                generality_rationale: None,
                ..Default::default()
            }],
        })
    }
}

#[derive(Clone)]
struct FailingExtractor;

#[async_trait]
impl TranscriptSkillExtractionService for FailingExtractor {
    async fn extract(
        &self,
        _transcript: &SessionTranscript,
    ) -> Result<ExtractionResult, ExtractionError> {
        Err(ExtractionError::Unexpected(
            "simulated extraction failure".to_owned(),
        ))
    }
}

#[derive(Clone, Default)]
struct CapturingEventPublisher {
    published_events: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl CapturingEventPublisher {
    fn list(&self) -> Vec<EventEnvelope> {
        self.published_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl ExtractionEventPublisher for CapturingEventPublisher {
    async fn publish(
        &self,
        envelope: &EventEnvelope,
    ) -> Result<(), session_extractor::LifecycleEventPublishError> {
        if let Ok(mut events) = self.published_events.lock() {
            events.push(envelope.clone());
        }
        Ok(())
    }
}

async fn wait_for_event(tool: &ExtractSessionTool, event_type: &str) {
    for _ in 0..60 {
        if tool
            .lifecycle_events()
            .iter()
            .any(|event| event.event_type == event_type)
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("event `{event_type}` was not emitted in time");
}

async fn wait_for_published_event(publisher: &CapturingEventPublisher, event_type: &str) {
    for _ in 0..60 {
        if publisher
            .list()
            .iter()
            .any(|event| event.event_type == event_type)
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("published event `{event_type}` was not observed in time");
}

#[tokio::test]
async fn extract_session_returns_processing_and_writes_pending_draft() {
    let sandbox = fresh_sandbox();
    let transcript_root = sandbox.join("transcripts");
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    let _allowlist_guard = allowlist_repo_root(&sandbox);
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");
    std::fs::copy(
        sample_fixture_path(),
        transcript_root.join("sample-transcript.jsonl"),
    )
    .expect("fixture should copy");

    let event_publisher = Arc::new(CapturingEventPublisher::default());
    let extractor = SessionExtractor::new_for_tests_with_publisher(
        ExtractionProvider::Claude,
        Arc::new(SuccessfulExtractor),
        TranscriptLoader::new(transcript_root.clone()).expect("loader should initialize"),
        PendingDraftWriter::new_unbounded_for_tests(vec![global_root.clone()]),
        event_publisher.clone(),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "sample-transcript.jsonl".to_owned(),
            transcript_inline: None,
            session_id: "session_extract_success".to_owned(),
            repo_path: Some(project_root.display().to_string()),
        })
        .await;

    assert_eq!(response.status, "processing");
    assert!(response.job_id.is_some());

    wait_for_event(&tool, "extraction.completed").await;
    wait_for_published_event(&event_publisher, "extraction.completed").await;
    let published_events = event_publisher.list();
    assert!(
        published_events
            .iter()
            .any(|event| event.event_type == "skill.extraction_requested")
    );
    assert!(
        published_events
            .iter()
            .any(|event| event.event_type == "extraction.completed")
    );
    assert!(
        !published_events
            .iter()
            .any(|event| event.event_type == "extraction.failed")
    );

    let pending_path = project_root.join(".skills/rust-file-io-setup/SKILL.md.pending");
    assert!(pending_path.exists(), "pending draft should be written");
    let pending_body = std::fs::read_to_string(&pending_path).expect("pending file should read");
    assert!(
        pending_body.contains("tags: [\"rust\", \"io\"]")
            || pending_body.contains("tags:\n- rust\n- io"),
        "pending frontmatter should retain extracted tags, got:\n{pending_body}"
    );
    assert!(pending_body.contains("origin: session_extraction"));
    assert!(pending_body.contains("# Rust File IO Setup"));
}

#[tokio::test]
async fn extract_session_rejects_repo_path_outside_allowed_roots() {
    let sandbox = fresh_sandbox();
    let transcript_root = sandbox.join("transcripts");
    let global_root = sandbox.join("global");
    let allowed_root = sandbox.join("allowed");
    let blocked_repo = sandbox.join("blocked").join("repo");
    let _allowlist_guard = allowlist_repo_root(&allowed_root);
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");
    std::fs::create_dir_all(&allowed_root).expect("allowed root should exist");
    std::fs::create_dir_all(&blocked_repo).expect("blocked repo should exist");
    std::fs::copy(
        sample_fixture_path(),
        transcript_root.join("sample-transcript.jsonl"),
    )
    .expect("fixture should copy");

    let extractor = SessionExtractor::new_for_tests(
        ExtractionProvider::Claude,
        Arc::new(SuccessfulExtractor),
        TranscriptLoader::new(transcript_root.clone()).expect("loader should initialize"),
        PendingDraftWriter::new_unbounded_for_tests(vec![global_root.clone()]),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "sample-transcript.jsonl".to_owned(),
            transcript_inline: None,
            session_id: "session_extract_invalid_repo_path".to_owned(),
            repo_path: Some(blocked_repo.display().to_string()),
        })
        .await;

    assert_eq!(response.status, "failed");
    assert_eq!(response.reason_code.as_deref(), Some("invalid_repo_path"));
    assert!(response.job_id.is_none());
    assert!(tool.lifecycle_events().is_empty());
}

#[tokio::test]
async fn extract_session_rejects_traversal_transcript_refs() {
    let sandbox = fresh_sandbox();
    let transcript_root = sandbox.join("transcripts");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");

    let extractor = SessionExtractor::new_for_tests(
        ExtractionProvider::Claude,
        Arc::new(SuccessfulExtractor),
        TranscriptLoader::new(transcript_root).expect("loader should initialize"),
        PendingDraftWriter::new_unbounded_for_tests(vec![global_root]),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "../host-transcript.jsonl".to_owned(),
            transcript_inline: None,
            session_id: "session_extract_reject".to_owned(),
            repo_path: None,
        })
        .await;

    assert_eq!(response.status, "failed");
    assert_eq!(
        response.reason_code.as_deref(),
        Some("invalid_transcript_ref")
    );
    assert!(response.job_id.is_none());
    assert!(tool.lifecycle_events().is_empty());
}

#[tokio::test]
async fn extract_session_emits_failed_lifecycle_event_on_background_error() {
    let sandbox = fresh_sandbox();
    let transcript_root = sandbox.join("transcripts");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");
    std::fs::copy(
        sample_fixture_path(),
        transcript_root.join("sample-transcript.jsonl"),
    )
    .expect("fixture should copy");

    let event_publisher = Arc::new(CapturingEventPublisher::default());
    let extractor = SessionExtractor::new_for_tests_with_publisher(
        ExtractionProvider::Ollama,
        Arc::new(FailingExtractor),
        TranscriptLoader::new(transcript_root).expect("loader should initialize"),
        PendingDraftWriter::new_unbounded_for_tests(vec![global_root]),
        event_publisher.clone(),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "sample-transcript.jsonl".to_owned(),
            transcript_inline: None,
            session_id: "session_extract_failure".to_owned(),
            repo_path: None,
        })
        .await;
    assert_eq!(response.status, "processing");

    wait_for_event(&tool, "extraction.failed").await;
    wait_for_published_event(&event_publisher, "extraction.failed").await;
    let events = event_publisher.list();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "skill.extraction_requested")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "extraction.failed")
    );
}

#[test]
fn providers_emit_same_contract_shape() {
    let session_id = DomainId::new_unchecked("contract-shape");
    let claude_result = ExtractionResult {
        source_session_id: session_id.clone(),
        provider: "claude".to_owned(),
        candidates: vec![ExtractedSkillCandidate {
            name: "sample".to_owned(),
            description: "desc".to_owned(),
            tags: vec!["tag".to_owned()],
            procedures: vec!["proc".to_owned()],
            conventions: vec!["conv".to_owned()],
            assets: vec!["asset".to_owned()],
            confidence: 0.9,
            generality: None,
            generality_rationale: None,
            ..Default::default()
        }],
    };
    let ollama_result = ExtractionResult {
        source_session_id: session_id,
        provider: "ollama".to_owned(),
        candidates: claude_result.candidates.clone(),
    };

    let claude_contract = session_extractor::extraction_contract_view(&claude_result);
    let ollama_contract = session_extractor::extraction_contract_view(&ollama_result);
    assert_eq!(
        claude_contract
            .get("candidates")
            .and_then(|value| value.as_array())
            .map(|candidates| candidates[0]
                .as_object()
                .expect("candidate should be object")
                .keys()
                .cloned()
                .collect::<Vec<_>>()),
        ollama_contract
            .get("candidates")
            .and_then(|value| value.as_array())
            .map(|candidates| candidates[0]
                .as_object()
                .expect("candidate should be object")
                .keys()
                .cloned()
                .collect::<Vec<_>>())
    );
}
