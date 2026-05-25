use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use domain::{
    DomainId, EmbeddingError, EmbeddingService, ExtractedSkillCandidate, ExtractionError,
    ExtractionResult, LifecycleStatus, ScopeType, SessionTranscript, Skill, SkillStatus, Subunit,
    SubunitType, TranscriptSkillExtractionService,
};
use infrastructure::EventEnvelope;
use mcp_server::{
    build_seeded_server,
    tools::{
        compile_context::{CompileContextRequest, CompileContextStatus},
        extract_session::{ExtractSessionRequest, ExtractSessionTool},
    },
};
use retrieval::{RetrievalConfig, SeededGraph, SeededSkill};
use session_extractor::{
    ExtractionEventPublisher, ExtractionProvider, SessionExtractor, transcripts::TranscriptLoader,
    writer::PendingDraftWriter,
};

#[path = "../integration/env_guard.rs"]
mod env_guard;

#[derive(Clone)]
struct DeterministicEmbeddingService;

impl DeterministicEmbeddingService {
    fn token_vector(&self, text: &str) -> Vec<f32> {
        let normalized = text.to_lowercase();
        let contains = |token: &str| normalized.contains(token);
        vec![
            if contains("rust") { 1.0 } else { 0.0 },
            if contains("auth") { 1.0 } else { 0.0 },
            if contains("global") { 1.0 } else { 0.0 },
            if contains("file") { 1.0 } else { 0.0 },
        ]
    }
}

#[async_trait]
impl EmbeddingService for DeterministicEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(self.token_vector(text))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts.iter().map(|text| self.token_vector(text)).collect())
    }
}

fn seeded_graph() -> SeededGraph {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize");
    let docs_root = repo_root.join("docs");

    let project_skill = Skill {
        id: DomainId::new_unchecked("skill-project-rust-auth"),
        name: "project-rust-auth-playbook".to_owned(),
        description: "Repository-specific Rust auth and file debugging workflow".to_owned(),
        scope: ScopeType::Project,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "auth".to_owned(), "project".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-project-auth")],
        community_id: None,
    };
    let global_skill = Skill {
        id: DomainId::new_unchecked("skill-global-rust-file"),
        name: "global-rust-file-patterns".to_owned(),
        description: "Global Rust file-handling patterns".to_owned(),
        scope: ScopeType::Global,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "file".to_owned(), "global".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-global-file")],
        community_id: None,
    };

    SeededGraph::new(
        vec![
            SeededSkill {
                skill: project_skill.clone(),
                scope_id: "project".to_owned(),
                source_paths: vec![repo_root.join("src/auth.rs")],
                embedding: vec![1.0, 1.0, 0.0, 1.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-project-auth"),
                    skill_id: project_skill.id.clone(),
                    kind: SubunitType::Procedure,
                    title: "Inspect auth middleware chain".to_owned(),
                    content: "Validate auth middleware ordering and file access guards.".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.2,
                community_boost: 0.3,
            },
            SeededSkill {
                skill: global_skill.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![docs_root.join("global-rust-file.md")],
                embedding: vec![1.0, 0.0, 1.0, 1.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-global-file"),
                    skill_id: global_skill.id.clone(),
                    kind: SubunitType::Convention,
                    title: "Return Result for file IO".to_owned(),
                    content: "Prefer explicit Result propagation for filesystem boundaries."
                        .to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.2,
            },
        ],
        13,
    )
}

fn retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 32,
        max_results: 2,
        max_subunits_per_skill: 4,
        rescue_threshold: 0.1,
        relevance_threshold: 0.15,
        mmr_lambda: 0.6,
        ..RetrievalConfig::default()
    }
}

fn test_repo_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
        .display()
        .to_string()
}

fn fresh_sandbox(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

#[derive(Clone, Default)]
struct CapturingEventPublisher {
    events: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl CapturingEventPublisher {
    fn list(&self) -> Vec<EventEnvelope> {
        self.events
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
        if let Ok(mut events) = self.events.lock() {
            events.push(envelope.clone());
        }
        Ok(())
    }
}

#[derive(Clone)]
struct InlineSuccessExtractor;

#[async_trait]
impl TranscriptSkillExtractionService for InlineSuccessExtractor {
    async fn extract(
        &self,
        transcript: &SessionTranscript,
    ) -> Result<ExtractionResult, ExtractionError> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(ExtractionResult {
            source_session_id: transcript.session_id.clone(),
            provider: "claude".to_owned(),
            candidates: vec![ExtractedSkillCandidate {
                name: "Session Extract Inline Workflow".to_owned(),
                description: "Validated inline transcript extraction flow.".to_owned(),
                tags: vec!["extraction".to_owned(), "inline".to_owned()],
                procedures: vec![
                    "Accept inline transcript JSONL and emit pending draft.".to_owned(),
                ],
                conventions: vec!["Publish requested/completed lifecycle events.".to_owned()],
                assets: vec!["tests/e2e/test_live_data_plane_roundtrip.rs".to_owned()],
                confidence: 0.95,
            }],
        })
    }
}

async fn wait_for_tool_event_count(tool: &ExtractSessionTool, event_type: &str, expected: usize) {
    for _ in 0..120 {
        let count = tool
            .lifecycle_events()
            .iter()
            .filter(|event| event.event_type == event_type)
            .count();
        if count >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("expected at least {expected} `{event_type}` events");
}

async fn wait_for_published_event_count(
    publisher: &CapturingEventPublisher,
    event_type: &str,
    expected: usize,
) {
    for _ in 0..120 {
        let count = publisher
            .list()
            .iter()
            .filter(|event| event.event_type == event_type)
            .count();
        if count >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("expected at least {expected} published `{event_type}` events");
}

fn inline_transcript_jsonl() -> String {
    r#"{"speaker":"user","content":"extract a rust workflow"}
{"speaker":"assistant","content":"here is a reusable flow"}"#
        .to_owned()
}

#[tokio::test]
async fn roundtrip_compile_context_returns_context_then_duplicate_suppression() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService),
        seeded_graph(),
        retrieval_config(),
    );

    let request = CompileContextRequest {
        prompt: "debug rust auth file access".to_owned(),
        session_id: "live-roundtrip".to_owned(),
        repo_path: test_repo_path(),
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::Ok);
    let markdown = first.additional_context.unwrap_or_default();
    assert!(markdown.contains("project-rust-auth-playbook"));
    assert!(markdown.contains("global-rust-file-patterns"));
    assert!(first.latency_ms < 1_000);

    let second = server.compile_context(request).await;
    assert_eq!(second.status, CompileContextStatus::DuplicateSuppressed);
    assert_eq!(
        second.reason_code.as_deref(),
        Some("already_compiled_for_session")
    );
}

#[tokio::test]
async fn invalid_repo_path_degrades_but_preserves_global_context_contract() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService),
        seeded_graph(),
        retrieval_config(),
    );
    let sandbox = fresh_sandbox("live-roundtrip-nonrepo");

    let response = server
        .compile_context(CompileContextRequest {
            prompt: "rust file patterns".to_owned(),
            session_id: "live-roundtrip-invalid-repo".to_owned(),
            repo_path: sandbox.display().to_string(),
        })
        .await;
    assert_eq!(response.status, CompileContextStatus::Degraded);
    assert_eq!(
        response.reason_code.as_deref(),
        Some("project_scope_resolution_failed")
    );
    let markdown = response.additional_context.unwrap_or_default();
    assert!(markdown.contains("global-rust-file-patterns"));

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[tokio::test]
async fn extract_session_inline_payload_writes_pending_and_emits_completion_events() {
    let sandbox = fresh_sandbox("live-roundtrip-extract-success");
    let transcript_root = sandbox.join("transcripts");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");

    let publisher = Arc::new(CapturingEventPublisher::default());
    let extractor = SessionExtractor::new_for_tests_with_publisher(
        ExtractionProvider::Claude,
        Arc::new(InlineSuccessExtractor),
        TranscriptLoader::new(transcript_root).expect("loader should initialize"),
        PendingDraftWriter::new(vec![global_root.clone()]),
        publisher.clone(),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "inline.jsonl".to_owned(),
            transcript_inline: Some(inline_transcript_jsonl()),
            session_id: "session_extract_inline".to_owned(),
            repo_path: None,
        })
        .await;
    assert_eq!(response.status, "processing");
    assert_eq!(response.provider.as_deref(), Some("claude"));
    assert!(response.job_id.is_some());

    wait_for_tool_event_count(&tool, "extraction.completed", 1).await;
    wait_for_published_event_count(&publisher, "extraction.completed", 1).await;

    let pending_path = global_root.join(".skills/session-extract-inline-workflow.pending");
    assert!(pending_path.exists(), "pending file should be written");
    let pending_body =
        std::fs::read_to_string(&pending_path).expect("pending file should be readable");
    assert!(pending_body.contains("origin: session_extraction"));
    assert!(pending_body.contains("# Session Extract Inline Workflow"));
    assert!(
        !tool
            .lifecycle_events()
            .iter()
            .any(|event| event.event_type == "extraction.failed")
    );

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[tokio::test]
async fn extract_session_invalid_inline_payload_surfaces_failed_event_without_pending_write() {
    let sandbox = fresh_sandbox("live-roundtrip-extract-failure");
    let transcript_root = sandbox.join("transcripts");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");

    let publisher = Arc::new(CapturingEventPublisher::default());
    let extractor = SessionExtractor::new_for_tests_with_publisher(
        ExtractionProvider::Claude,
        Arc::new(InlineSuccessExtractor),
        TranscriptLoader::new(transcript_root).expect("loader should initialize"),
        PendingDraftWriter::new(vec![global_root.clone()]),
        publisher.clone(),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let response = tool
        .invoke(ExtractSessionRequest {
            transcript_ref: "inline.jsonl".to_owned(),
            transcript_inline: Some("not-json".to_owned()),
            session_id: "session_extract_inline_invalid".to_owned(),
            repo_path: None,
        })
        .await;
    assert_eq!(response.status, "processing");

    wait_for_tool_event_count(&tool, "extraction.failed", 1).await;
    wait_for_published_event_count(&publisher, "extraction.failed", 1).await;
    assert_eq!(
        tool.lifecycle_events()
            .iter()
            .filter(|event| event.event_type == "extraction.completed")
            .count(),
        0
    );

    let pending_root = global_root.join(".skills");
    if pending_root.exists() {
        let pending_count = std::fs::read_dir(&pending_root)
            .expect("pending root should be readable")
            .filter_map(Result::ok)
            .count();
        assert_eq!(pending_count, 0, "no pending files should be created");
    }

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}
