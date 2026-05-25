use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use domain::{
    DomainId, EmbeddingError, EmbeddingService, LifecycleStatus, ScopeType, Skill, SkillStatus,
    Subunit, SubunitType,
};
use mcp_server::{
    build_seeded_server,
    protocol::JsonRpcRequest,
    tools::{
        compile_context::{CompileContextRequest, CompileContextStatus},
        find_skill::FindSkillRequest,
    },
};
use retrieval::{RetrievalConfig, SeededGraph, SeededSkill};
use serde_json::json;

#[path = "env_guard.rs"]
mod env_guard;

#[derive(Clone)]
struct DeterministicEmbeddingService {
    fail_next: Arc<AtomicUsize>,
}

impl DeterministicEmbeddingService {
    fn healthy() -> Self {
        Self {
            fail_next: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn fail_first() -> Self {
        Self {
            fail_next: Arc::new(AtomicUsize::new(1)),
        }
    }

    fn embed_internal(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if self.fail_next.load(Ordering::SeqCst) > 0 {
            self.fail_next.fetch_sub(1, Ordering::SeqCst);
            return Err(EmbeddingError::ProviderUnavailable(
                "seeded provider outage".to_owned(),
            ));
        }

        let normalized = text.to_lowercase();
        let tokens: Vec<&str> = normalized
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|token| !token.is_empty())
            .collect();
        let contains = |token: &str| tokens.iter().any(|candidate| candidate == &token);

        Ok(vec![
            if contains("rust") { 1.0 } else { 0.0 },
            if contains("file") || contains("read") {
                1.0
            } else {
                0.0
            },
            if contains("async") || contains("tokio") {
                1.0
            } else {
                0.0
            },
            if contains("python") { 1.0 } else { 0.0 },
        ])
    }
}

#[async_trait]
impl EmbeddingService for DeterministicEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed_internal(text)
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        texts
            .iter()
            .map(|text| self.embed_internal(text))
            .collect::<Result<Vec<Vec<f32>>, EmbeddingError>>()
    }
}

fn seeded_graph() -> SeededGraph {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize");
    let docs_root = repo_root.join("docs");

    let rust_skill = Skill {
        id: DomainId::new_unchecked("skill-rust-file"),
        name: "rust-file-reading".to_owned(),
        description: "Read file contents safely in Rust".to_owned(),
        scope: ScopeType::Global,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "io".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-rust-file-read")],
        community_id: None,
    };
    let tokio_skill = Skill {
        id: DomainId::new_unchecked("skill-tokio-io"),
        name: "tokio-io-primitives".to_owned(),
        description: "Async filesystem and runtime-safe IO patterns".to_owned(),
        scope: ScopeType::Global,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "tokio".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-tokio-file-read")],
        community_id: None,
    };
    let python_skill = Skill {
        id: DomainId::new_unchecked("skill-python"),
        name: "python-requests".to_owned(),
        description: "HTTP requests in python".to_owned(),
        scope: ScopeType::Global,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["python".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-python-http")],
        community_id: None,
    };

    SeededGraph::new(
        vec![
            SeededSkill {
                skill: rust_skill.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![docs_root.join("rust-file.md")],
                embedding: vec![1.0, 1.0, 0.0, 0.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-rust-file-read"),
                    skill_id: rust_skill.id.clone(),
                    kind: SubunitType::Procedure,
                    title: "Read file to string".to_owned(),
                    content: "Use std::fs::read_to_string(path) and handle Result.".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.3,
            },
            SeededSkill {
                skill: tokio_skill.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![docs_root.join("tokio-io.md")],
                embedding: vec![0.9, 0.5, 1.0, 0.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-tokio-file-read"),
                    skill_id: tokio_skill.id.clone(),
                    kind: SubunitType::Procedure,
                    title: "Async file read".to_owned(),
                    content: "Use tokio::fs::read_to_string for async workloads.".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.2,
            },
            SeededSkill {
                skill: python_skill.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![docs_root.join("python-http.md")],
                embedding: vec![0.0, 0.0, 0.0, 1.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-python-http"),
                    skill_id: python_skill.id.clone(),
                    kind: SubunitType::Procedure,
                    title: "Make HTTP requests".to_owned(),
                    content: "Use requests.get and check response codes.".to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.1,
            },
        ],
        7,
    )
}

fn retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 20,
        max_results: 1,
        max_subunits_per_skill: 3,
        rescue_threshold: 0.1,
        relevance_threshold: 0.25,
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

#[tokio::test]
async fn registers_compile_context_find_skill_and_extract_session_tools() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
    );
    assert_eq!(
        server.registered_tools(),
        &[
            "compile_context".to_owned(),
            "extract_session".to_owned(),
            "find_skill".to_owned()
        ]
    );
}

#[tokio::test]
async fn compile_context_returns_ok_then_duplicate_suppressed_after_healthy_result() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
    );

    let request = CompileContextRequest {
        prompt: "how do i read a file in rust".to_owned(),
        session_id: "session-ok".to_owned(),
        repo_path: test_repo_path(),
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::Ok);
    assert!(first.additional_context.is_some());
    let markdown = first.additional_context.unwrap_or_default();
    assert!(markdown.contains("rust-file-reading"));
    assert!(markdown.contains("### Highlights"));
    assert!(markdown.contains("### Rescue cues"));
    assert!(first.latency_ms < 500);

    let second = server.compile_context(request).await;
    assert_eq!(second.status, CompileContextStatus::DuplicateSuppressed);
    assert_eq!(
        second.reason_code.as_deref(),
        Some("already_compiled_for_session")
    );
    assert_eq!(second.graph_version, first.graph_version);
    assert_eq!(second.scopes_considered, first.scopes_considered);
}

#[tokio::test]
async fn compile_context_returns_no_match_for_healthy_empty_and_suppresses_followups() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
    );

    let request = CompileContextRequest {
        prompt: "quantum banana".to_owned(),
        session_id: "session-empty".to_owned(),
        repo_path: test_repo_path(),
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::NoMatch);
    assert_eq!(first.reason_code.as_deref(), Some("no_relevant_skills"));
    assert!(first.additional_context.is_none());

    let second = server.compile_context(request).await;
    assert_eq!(second.status, CompileContextStatus::DuplicateSuppressed);
    assert_eq!(second.graph_version, first.graph_version);
    assert_eq!(second.scopes_considered, first.scopes_considered);
}

#[tokio::test]
async fn degraded_first_attempt_does_not_set_suppression_state() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::fail_first()),
        seeded_graph(),
        retrieval_config(),
    );

    let request = CompileContextRequest {
        prompt: "how do i read a file in rust".to_owned(),
        session_id: "session-degraded".to_owned(),
        repo_path: test_repo_path(),
    };

    let degraded = server.compile_context(request.clone()).await;
    assert_eq!(degraded.status, CompileContextStatus::Degraded);
    assert_eq!(
        degraded.reason_code.as_deref(),
        Some("embedding_provider_unavailable")
    );

    let retry = server.compile_context(request).await;
    assert_eq!(retry.status, CompileContextStatus::Ok);
}

#[tokio::test]
async fn find_skill_reports_top_matches_from_seeded_graph() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
    );

    let response = server
        .find_skill(FindSkillRequest {
            prompt: "tokio async file read".to_owned(),
            limit: Some(1),
        })
        .await;

    assert_eq!(response.status, "ok");
    assert_eq!(response.matches.len(), 1);
}

#[tokio::test]
async fn json_rpc_tools_list_and_call_compile_context() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
    );

    let tools_list = server
        .handle_json_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: json!({}),
        })
        .await;
    let tools = tools_list
        .result
        .as_ref()
        .and_then(|result| result.get("tools"))
        .and_then(|tools| tools.as_array())
        .expect("tools/list result should include tools array");
    assert!(
        tools
            .iter()
            .any(|tool| tool.get("name") == Some(&json!("compile_context")))
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool.get("name") == Some(&json!("find_skill")))
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool.get("name") == Some(&json!("extract_session")))
    );

    let call_response = server
        .handle_json_rpc(JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2)),
            method: "tools/call".to_owned(),
            params: json!({
                "name": "compile_context",
                "arguments": {
                    "prompt": "how do i read a file in rust",
                    "session_id": "session-rpc",
                    "repo_path": test_repo_path()
                }
            }),
        })
        .await;

    let status = call_response
        .result
        .as_ref()
        .and_then(|result| result.get("status"))
        .and_then(|value| value.as_str());
    assert_eq!(status, Some("ok"));
}
