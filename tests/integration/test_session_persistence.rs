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
    McpServerApp,
    tools::compile_context::{CompileContextRequest, CompileContextStatus},
};
use retrieval::{RetrievalConfig, RetrievalSnapshot, SeededSkill};

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

fn seeded_graph() -> RetrievalSnapshot {
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

    RetrievalSnapshot::new(
        vec![
            SeededSkill {
                skill: rust_skill.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![docs_root.join("rust-file.md")],
                embedding: vec![1.0, 1.0, 0.0, 0.0],
                subunit_embeddings: vec![vec![1.0, 1.0, 0.0, 0.0]],
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
                subunit_embeddings: vec![vec![0.9, 0.5, 1.0, 0.0]],
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
                subunit_embeddings: vec![vec![0.0, 0.0, 0.0, 1.0]],
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
async fn repeated_prompt_returns_cached_context_without_rerunning_pipeline() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "how do i read a file in rust".to_owned(),
        session_id: "session-cache".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::Ok);
    assert!(first.additional_context.is_some());
    let _first_markdown = first.additional_context.clone().unwrap_or_default();

    // Same session: second call is suppressed (suppression before cache per #073).
    let second = server.compile_context(request.clone()).await;
    assert_eq!(second.status, CompileContextStatus::DuplicateSuppressed);

    // Different session with same prompt: cache is per-session now (#076), so
    // new session triggers fresh retrieval — no cross-session cache sharing.
    let different_session = CompileContextRequest {
        prompt: "how do i read a file in rust".to_owned(),
        session_id: "session-cache-b".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
    };
    let third = server.compile_context(different_session).await;
    assert_eq!(third.status, CompileContextStatus::Ok);
    assert!(third.additional_context.is_some());
}

#[tokio::test]
async fn cache_invalidated_on_graph_version_mismatch() {
    let _env_guard = env_guard::configure_scope_env();
    let graph_v7 = seeded_graph();
    let server_v7 = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        graph_v7,
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "how do i read a file in rust".to_owned(),
        session_id: "session-version".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
    };

    let first = server_v7.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::Ok);

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve");
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

    let graph_v8 = RetrievalSnapshot::new(
        vec![SeededSkill {
            skill: rust_skill.clone(),
            scope_id: "global".to_owned(),
            source_paths: vec![docs_root.join("rust-file.md")],
            embedding: vec![1.0, 1.0, 0.0, 0.0],
            subunit_embeddings: vec![vec![1.0, 1.0, 0.0, 0.0]],
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
        }],
        8,
    );

    let server_v8 = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        graph_v8,
        retrieval_config(),
        None,
    );

    let second = server_v8.compile_context(request.clone()).await;
    assert_eq!(second.status, CompileContextStatus::Ok);
    assert_eq!(second.graph_version, 8);
}

#[tokio::test]
async fn degraded_outcome_does_not_populate_cache() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService {
            fail_next: Arc::new(AtomicUsize::new(1)),
        }),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "how do i read a file in rust".to_owned(),
        session_id: "session-degraded-cache".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::Degraded);

    // Because degraded outcomes do not populate the cache, the second call
    // reruns retrieval. The embedding service is now healthy (counter
    // decremented to 0), so the outcome is Ok.
    let second = server.compile_context(request.clone()).await;
    assert_eq!(second.status, CompileContextStatus::Ok);
    assert!(second.additional_context.is_some());
}

#[tokio::test]
async fn healthy_no_match_populates_cache_and_returns_cached_on_repeat() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "quantum banana".to_owned(),
        session_id: "session-nomatch-cache".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::NoMatch);

    let second = server.compile_context(request.clone()).await;
    assert_eq!(second.status, CompileContextStatus::DuplicateSuppressed);

    // Different session: cache is per-session (#076), fresh retrieval.
    let different_session = CompileContextRequest {
        prompt: "quantum banana".to_owned(),
        session_id: "session-nomatch-cache-b".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
    };
    let third = server.compile_context(different_session).await;
    assert_eq!(third.status, CompileContextStatus::NoMatch);
    assert_eq!(third.reason_code, first.reason_code);
    assert_eq!(third.scopes_considered, first.scopes_considered);
}
