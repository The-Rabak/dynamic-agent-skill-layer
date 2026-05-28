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
    tools::compile_context::{CompileContextRequest, CompileContextStatus},
};
use retrieval::{RetrievalConfig, SeededGraph, SeededSkill};

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
        let contains = |token: &str| normalized.contains(token);

        Ok(vec![
            if contains("rust") { 1.0 } else { 0.0 },
            if contains("auth") { 1.0 } else { 0.0 },
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

fn seeded_dual_scope_graph() -> SeededGraph {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize");
    let docs_root = repo_root.join("docs");

    let project_skill = Skill {
        id: DomainId::new_unchecked("skill-project-rust-auth"),
        name: "project-rust-auth-playbook".to_owned(),
        description: "Repository-specific Rust auth debugging workflow".to_owned(),
        scope: ScopeType::Project,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "auth".to_owned(), "project".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-project-rust-auth")],
        community_id: None,
    };

    let global_skill = Skill {
        id: DomainId::new_unchecked("skill-global-rust-auth"),
        name: "global-rust-auth-patterns".to_owned(),
        description: "Cross-project Rust authentication conventions".to_owned(),
        scope: ScopeType::Global,
        status: SkillStatus::Ready,
        lifecycle: LifecycleStatus::Active,
        tags: vec!["rust".to_owned(), "auth".to_owned(), "global".to_owned()],
        subunit_ids: vec![DomainId::new_unchecked("sub-global-rust-auth")],
        community_id: None,
    };

    SeededGraph::new(
        vec![
            SeededSkill {
                skill: project_skill.clone(),
                scope_id: "project".to_owned(),
                source_paths: vec![repo_root.join("src/auth.rs")],
                embedding: vec![1.0, 1.0, 0.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-project-rust-auth"),
                    skill_id: project_skill.id.clone(),
                    kind: SubunitType::Procedure,
                    title: "Inspect project auth middleware".to_owned(),
                    content: "Trace middleware order and repository-specific policy checks."
                        .to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.2,
                community_boost: 0.3,
            },
            SeededSkill {
                skill: global_skill.clone(),
                scope_id: "global".to_owned(),
                source_paths: vec![docs_root.join("global-rust-auth.md")],
                embedding: vec![0.9, 1.0, 0.0],
                subunits: vec![Subunit {
                    id: DomainId::new_unchecked("sub-global-rust-auth"),
                    skill_id: global_skill.id.clone(),
                    kind: SubunitType::Convention,
                    title: "Validate auth token lifetime".to_owned(),
                    content: "Check expiration handling and token rotation best practices."
                        .to_owned(),
                    lifecycle: LifecycleStatus::Active,
                }],
                prior: 0.1,
                community_boost: 0.2,
            },
        ],
        9,
    )
}

fn retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 20,
        max_results: 2,
        max_subunits_per_skill: 3,
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

fn configure_scope_env_with_missing_global_path() -> env_guard::ScopeEnvGuard {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve");
    env_guard::configure_scope_env_with_global_path(repo_root.join("docs/missing-global-scope"))
}

#[tokio::test]
async fn compile_context_searches_project_and_global_with_project_priority_bias() {
    let _env_guard = env_guard::configure_scope_env();

    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_dual_scope_graph(),
        retrieval_config(),
        None,
    );

    let response = server
        .compile_context(CompileContextRequest {
            prompt: "debug rust auth middleware".to_owned(),
            session_id: "dual-scope-session".to_owned(),
            repo_path: test_repo_path(),
        })
        .await;

    assert_eq!(response.status, CompileContextStatus::Ok);
    assert_eq!(response.scopes_considered, vec!["project", "global"]);

    let markdown = response.additional_context.unwrap_or_default();
    assert!(markdown.contains("project-rust-auth-playbook"));
    assert!(markdown.contains("global-rust-auth-patterns"));

    let project_idx = markdown
        .find("project-rust-auth-playbook")
        .expect("project skill should appear");
    let global_idx = markdown
        .find("global-rust-auth-patterns")
        .expect("global skill should appear");

    assert!(
        project_idx < global_idx,
        "project scope should be favored in fused ordering"
    );
}

#[tokio::test]
async fn suppression_is_scoped_by_session_and_repo_pair_and_degraded_does_not_consume_first_prompt()
{
    let _env_guard = env_guard::configure_scope_env();

    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::fail_first()),
        seeded_dual_scope_graph(),
        retrieval_config(),
        None,
    );

    let base_request = CompileContextRequest {
        prompt: "debug rust auth middleware".to_owned(),
        session_id: "session-a".to_owned(),
        repo_path: test_repo_path(),
    };

    let degraded = server.compile_context(base_request.clone()).await;
    assert_eq!(degraded.status, CompileContextStatus::Degraded);

    let retry = server.compile_context(base_request.clone()).await;
    assert_eq!(retry.status, CompileContextStatus::Ok);

    let isolated_repo = server
        .compile_context(CompileContextRequest {
            repo_path: format!("{}/crates", test_repo_path()),
            ..base_request.clone()
        })
        .await;
    assert_eq!(isolated_repo.status, CompileContextStatus::Ok);

    let isolated_session = server
        .compile_context(CompileContextRequest {
            session_id: "session-b".to_owned(),
            repo_path: test_repo_path(),
            ..base_request.clone()
        })
        .await;
    assert_eq!(isolated_session.status, CompileContextStatus::Ok);

    let duplicate = server.compile_context(base_request).await;
    assert_eq!(duplicate.status, CompileContextStatus::DuplicateSuppressed);
}

#[tokio::test]
async fn compile_context_uses_request_repo_path_for_scope_resolution() {
    let _env_guard = env_guard::configure_scope_env();
    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_dual_scope_graph(),
        retrieval_config(),
        None,
    );

    let valid_repo = test_repo_path();
    let sandbox = std::env::temp_dir().join(format!(
        "dual-scope-nonrepo-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");

    let valid_response = server
        .compile_context(CompileContextRequest {
            prompt: "debug rust auth middleware".to_owned(),
            session_id: "repo-scope-valid".to_owned(),
            repo_path: valid_repo,
        })
        .await;
    assert_eq!(valid_response.status, CompileContextStatus::Ok);
    assert_eq!(valid_response.scopes_considered, vec!["project", "global"]);

    let invalid_response = server
        .compile_context(CompileContextRequest {
            prompt: "debug rust auth middleware".to_owned(),
            session_id: "repo-scope-invalid".to_owned(),
            repo_path: sandbox.display().to_string(),
        })
        .await;
    assert_eq!(invalid_response.status, CompileContextStatus::Degraded);
    assert_eq!(
        invalid_response.reason_code.as_deref(),
        Some("project_scope_resolution_failed")
    );

    std::fs::remove_dir_all(sandbox).expect("sandbox cleanup should succeed");
}

#[tokio::test]
async fn partial_scope_failure_returns_degraded_with_available_context_and_no_suppression() {
    let _env_guard = configure_scope_env_with_missing_global_path();

    let server = build_seeded_server(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_dual_scope_graph(),
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "debug rust auth middleware".to_owned(),
        session_id: "partial-scope-failure".to_owned(),
        repo_path: test_repo_path(),
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::Degraded);
    assert_eq!(
        first.reason_code.as_deref(),
        Some("global_scope_resolution_failed")
    );

    let markdown = first
        .additional_context
        .expect("partial degraded result should return project context");
    assert!(markdown.contains("project-rust-auth-playbook"));
    assert!(!markdown.contains("global-rust-auth-patterns"));

    let second = server.compile_context(request).await;
    assert_eq!(second.status, CompileContextStatus::Degraded);
}
