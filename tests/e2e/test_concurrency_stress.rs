use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use domain::{
    DomainId, EmbeddingError, EmbeddingService, ExtractedSkillCandidate, ExtractionError,
    ExtractionResult, LifecycleStatus, PENDING_SKILL_FILE_NAME, ScopeType, SessionTranscript,
    Skill, SkillStatus, Subunit, SubunitType, TranscriptSkillExtractionService,
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
use tokio::task::JoinSet;

#[path = "../integration/env_guard.rs"]
mod env_guard;

#[derive(Clone)]
struct BurstEmbeddingService;

impl BurstEmbeddingService {
    fn embed_internal(&self, text: &str) -> Vec<f32> {
        let normalized = text.to_lowercase();
        let contains = |token: &str| normalized.contains(token);
        vec![
            if contains("rust") { 1.0 } else { 0.0 },
            if contains("auth") { 1.0 } else { 0.0 },
            if contains("file") { 1.0 } else { 0.0 },
            if contains("async") { 1.0 } else { 0.0 },
        ]
    }
}

#[async_trait]
impl EmbeddingService for BurstEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(self.embed_internal(text))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        Ok(texts.iter().map(|text| self.embed_internal(text)).collect())
    }
}

fn seeded_graph() -> SeededGraph {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize");
    let docs_root = repo_root.join("docs");

    let project_skill = Skill {
        id: DomainId::new_unchecked("skill-project-rust-file"),
        name: "project-rust-file-safety".to_owned(),
        description: "Project-specific Rust file access safety checks".to_owned(),
        scope: ScopeType::Project,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "project".to_owned(), "file".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-project-file-safety")],
        community_id: None,
    };
    let global_skill = Skill {
        id: DomainId::new_unchecked("skill-global-async-rust"),
        name: "global-async-rust-patterns".to_owned(),
        description: "Global async Rust conventions".to_owned(),
        scope: ScopeType::Global,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "async".to_owned(), "global".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-global-async")],
        community_id: None,
    };

    SeededGraph::new(
        vec![
            SeededSkill {
                skill: project_skill.clone(),
                scope_id: "project".to_owned(),
                source_paths: vec![repo_root.join("src/file_access.rs")],
                embedding: vec![1.0, 0.8, 1.0, 0.2],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-project-file-safety"),
                    skill_id: project_skill.id.clone(),
                    kind: SubunitType::Procedure,
                    title: "Gate file IO by policy".to_owned(),
                    content: "Apply project policy before reading or writing files.".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.2,
                community_boost: 0.3,
            },
            SeededSkill {
                skill: global_skill.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![docs_root.join("global-async-rust.md")],
                embedding: vec![1.0, 0.0, 0.4, 1.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-global-async"),
                    skill_id: global_skill.id.clone(),
                    kind: SubunitType::Convention,
                    title: "Preserve async boundaries".to_owned(),
                    content: "Avoid blocking calls in async Rust handlers.".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.2,
            },
        ],
        17,
    )
}

fn retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 32,
        max_results: 2,
        max_subunits_per_skill: 4,
        rescue_threshold: 0.1,
        relevance_threshold: 0.2,
        mmr_lambda: 0.55,
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
        .expect("system clock should be after epoch")
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
struct StressExtractor;

#[async_trait]
impl TranscriptSkillExtractionService for StressExtractor {
    async fn extract(
        &self,
        transcript: &SessionTranscript,
    ) -> Result<ExtractionResult, ExtractionError> {
        tokio::time::sleep(Duration::from_millis(8)).await;
        let session_label = transcript.session_id.as_str().replace('-', " ");
        Ok(ExtractionResult {
            source_session_id: transcript.session_id.clone(),
            provider: "claude".to_owned(),
            candidates: vec![ExtractedSkillCandidate {
                name: format!("Stress Extracted Skill {session_label}"),
                description: "Stress extraction output candidate".to_owned(),
                tags: vec!["stress".to_owned(), "e2e".to_owned()],
                procedures: vec!["Emit deterministic pending draft during burst load.".to_owned()],
                conventions: vec!["Never silently drop extraction lifecycle events.".to_owned()],
                assets: vec!["tests/e2e/test_concurrency_stress.rs".to_owned()],
                confidence: 0.87,
            }],
        })
    }
}

async fn wait_for_tool_event_count(tool: &ExtractSessionTool, event_type: &str, expected: usize) {
    for _ in 0..200 {
        let count = tool
            .lifecycle_events()
            .iter()
            .filter(|event| event.event_type == event_type)
            .count();
        if count >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("expected at least {expected} `{event_type}` events");
}

async fn wait_for_published_event_count(
    publisher: &CapturingEventPublisher,
    event_type: &str,
    expected: usize,
) {
    for _ in 0..200 {
        let count = publisher
            .list()
            .iter()
            .filter(|event| event.event_type == event_type)
            .count();
        if count >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("expected at least {expected} published `{event_type}` events");
}

fn inline_payload(index: usize) -> String {
    format!(
        r#"{{"speaker":"user","content":"stress transcript {index}"}}
{{"speaker":"assistant","content":"reusable flow {index}"}}"#
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn extract_session_parallel_burst_completes_all_jobs_and_persists_drafts() {
    let sandbox = fresh_sandbox("e2e-concurrency-extract");
    let transcript_root = sandbox.join("transcripts");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&transcript_root).expect("transcript root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");

    let publisher = Arc::new(CapturingEventPublisher::default());
    let extractor = SessionExtractor::new_for_tests_with_publisher(
        ExtractionProvider::Claude,
        Arc::new(StressExtractor),
        TranscriptLoader::new(transcript_root).expect("loader should initialize"),
        PendingDraftWriter::new(vec![global_root.clone()]),
        publisher.clone(),
    );
    let tool = ExtractSessionTool::new_for_tests(extractor);

    let request_count = 32usize;
    let mut set = JoinSet::new();
    for i in 0..request_count {
        let tool_clone = tool.clone();
        set.spawn(async move {
            tool_clone
                .invoke(ExtractSessionRequest {
                    transcript_ref: "inline.jsonl".to_owned(),
                    transcript_inline: Some(inline_payload(i)),
                    session_id: format!("stress-session-{i:02}"),
                    repo_path: None,
                })
                .await
        });
    }

    let mut job_ids = HashSet::new();
    let mut processed = 0usize;
    while let Some(result) = set.join_next().await {
        let response = result.expect("task should finish without panic");
        assert_eq!(response.status, "processing");
        assert_eq!(response.provider.as_deref(), Some("claude"));
        let job_id = response
            .job_id
            .expect("processing responses should include a job id");
        job_ids.insert(job_id);
        processed += 1;
    }
    assert_eq!(processed, request_count);
    assert_eq!(
        job_ids.len(),
        request_count,
        "every extraction request should produce a unique job id"
    );

    wait_for_tool_event_count(&tool, "extraction.completed", request_count).await;
    wait_for_published_event_count(&publisher, "extraction.completed", request_count).await;
    assert_eq!(
        tool.lifecycle_events()
            .iter()
            .filter(|event| event.event_type == "extraction.failed")
            .count(),
        0
    );

    let pending_root = global_root.join(".skills");
    let mut pending_count = 0usize;
    let mut noncanonical_pending_paths = Vec::new();
    let mut directories_to_scan = vec![pending_root.clone()];
    while let Some(directory) = directories_to_scan.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("pending directory should be readable: {error}"))
        {
            let entry = entry.expect("pending directory entry should be readable");
            let entry_path = entry.path();
            let file_type = entry
                .file_type()
                .expect("pending directory entry type should be readable");
            if file_type.is_dir() {
                directories_to_scan.push(entry_path);
                continue;
            }
            let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !file_name.ends_with(".pending") {
                continue;
            }
            if file_name == PENDING_SKILL_FILE_NAME {
                pending_count += 1;
            } else {
                noncanonical_pending_paths.push(entry.path());
            }
        }
    }
    assert!(
        noncanonical_pending_paths.is_empty(),
        "pending drafts must use canonical file name `{PENDING_SKILL_FILE_NAME}`; found {:?}",
        noncanonical_pending_paths
    );
    assert_eq!(pending_count, request_count);

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}
