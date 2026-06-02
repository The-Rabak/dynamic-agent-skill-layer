use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use admin::tools::{RebuildGraphRequest, RebuildGraphStatusRequest};
use async_trait::async_trait;
use domain::{
    DomainId, EmbeddingError, EmbeddingService, LifecycleStatus, ScopeType, Skill, SkillStatus,
    Subunit, SubunitType,
};
use mcp_server::{
    McpServerApp,
    protocol::{JsonRpcRequest, registered_tool_descriptors},
    tools::{
        compile_context::{CompileContextRequest, CompileContextStatus, TriggerKind},
        find_skill::FindSkillRequest,
    },
};
use retrieval::{RetrievalConfig, RetrievalSnapshot, SeededSkill};
use serde_json::json;

#[path = "env_guard.rs"]
mod env_guard;

struct DatabaseUrlGuard {
    previous: Option<std::ffi::OsString>,
}

impl DatabaseUrlGuard {
    fn unset() -> Self {
        let previous = std::env::var_os("DATABASE_URL");
        // SAFETY: integration tests mutate process env in scoped guard usage only.
        unsafe {
            std::env::remove_var("DATABASE_URL");
        }
        Self { previous }
    }
}

impl Drop for DatabaseUrlGuard {
    fn drop(&mut self) {
        // SAFETY: integration tests mutate process env in scoped guard usage only.
        unsafe {
            if let Some(previous) = &self.previous {
                std::env::set_var("DATABASE_URL", previous);
            } else {
                std::env::remove_var("DATABASE_URL");
            }
        }
    }
}

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

fn fresh_sandbox(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    std::fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

fn write_skill_file(root: &std::path::Path, slug: &str, title: &str) {
    let skill_dir = root.join(slug);
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be creatable");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            r#"# {title}

## Description
Reusable capability for {title}.

## Tags
- rust
- integration

## Procedure
1. Validate input.
2. Return deterministic output.
"#
        ),
    )
    .expect("skill file should be writable");
}

#[tokio::test]
async fn registers_compile_context_find_skill_and_extract_session_tools() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
    );
    assert_eq!(
        server.registered_tools(),
        vec![
            "compile_context",
            "find_skill",
            "extract_session",
            "rebuild_graph",
            "rebuild_graph_status",
            "inspect_skill",
            "list_communities"
        ]
    );
}

#[tokio::test]
async fn rebuild_graph_requires_live_graph_database_for_seeded_server() {
    let _database_url_guard = DatabaseUrlGuard::unset();
    let sandbox = fresh_sandbox("compile-context-rebuild");
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::create_dir_all(&global_root).expect("global root should exist");
    write_skill_file(&project_root, "project-skill", "Project Skill");
    write_skill_file(&global_root, "global-skill", "Global Skill");
    let _env_guard = env_guard::configure_scope_env_with_graph_builder_roots(
        global_root.clone(),
        Some(project_root),
        Some(global_root),
    );
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let response = server.rebuild_graph(RebuildGraphRequest::default()).await;
    assert_eq!(response.status, "accepted");
    let job_id = response
        .job_id
        .clone()
        .expect("accepted response should include job id");

    let mut reason_code = None;
    for _attempt in 0..50 {
        let status = server
            .rebuild_graph_status(RebuildGraphStatusRequest {
                job_id: job_id.clone(),
            })
            .await;
        let lifecycle = status
            .job
            .as_ref()
            .map(|job| job.lifecycle_status.as_str())
            .unwrap_or_default()
            .to_owned();
        reason_code = status.job.as_ref().and_then(|job| job.reason_code.clone());
        if lifecycle == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert_eq!(reason_code.as_deref(), Some("rebuild_unavailable"));
}

#[tokio::test]
async fn compile_context_returns_ok_then_duplicate_suppressed_after_healthy_result() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "how do i read a file in rust".to_owned(),
        session_id: "session-ok".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
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
    assert!(second.additional_context.is_none());
    assert_eq!(second.graph_version, first.graph_version);
    assert_eq!(second.scopes_considered, first.scopes_considered);
}

#[tokio::test]
async fn compile_context_returns_no_match_for_healthy_empty_and_suppresses_followups() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "quantum banana".to_owned(),
        session_id: "session-empty".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
    };

    let first = server.compile_context(request.clone()).await;
    assert_eq!(first.status, CompileContextStatus::NoMatch);
    assert_eq!(first.reason_code.as_deref(), Some("no_relevant_skills"));
    assert!(first.additional_context.is_none());

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
async fn degraded_first_attempt_does_not_set_suppression_state() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::fail_first()),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let request = CompileContextRequest {
        prompt: "how do i read a file in rust".to_owned(),
        session_id: "session-degraded".to_owned(),
        repo_path: test_repo_path(),
        trigger: None,
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
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
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
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
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
    let listed_tool_names = tools
        .iter()
        .map(|tool| {
            tool.get("name")
                .and_then(|name| name.as_str())
                .expect("tool should expose a string name")
        })
        .collect::<Vec<&str>>();
    assert_eq!(listed_tool_names, server.registered_tools());
    let canonical_required_arguments = registered_tool_descriptors()
        .into_iter()
        .map(|tool| (tool.name, tool.required_arguments))
        .collect::<std::collections::BTreeMap<&str, &[&str]>>();
    for listed_tool in tools {
        let tool_name = listed_tool
            .get("name")
            .and_then(|name| name.as_str())
            .expect("listed tool should have a name");
        let listed_required = listed_tool
            .pointer("/inputSchema/required")
            .and_then(|required| required.as_array())
            .expect("listed tool should expose required arguments")
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .expect("required argument should serialize to string")
            })
            .collect::<Vec<&str>>();
        let canonical_required = canonical_required_arguments
            .get(tool_name)
            .expect("listed tool should exist in canonical registry")
            .to_vec();
        assert_eq!(listed_required, canonical_required);

        let properties = listed_tool
            .pointer("/inputSchema/properties")
            .and_then(|props| props.as_object());
        assert!(
            properties.is_some(),
            "{tool_name}: inputSchema should include properties object"
        );
        let properties = properties.unwrap();
        for (prop_name, prop_value) in properties {
            assert!(
                prop_value.get("type").is_some(),
                "{tool_name}.{prop_name}: property should include type"
            );
            assert!(
                prop_value.get("description").is_some(),
                "{tool_name}.{prop_name}: property should include description"
            );
        }
    }

    let extract_schema = tools
        .iter()
        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some("extract_session"))
        .expect("extract_session should be listed")
        .get("inputSchema")
        .expect("extract_session should have inputSchema");
    let extract_properties = extract_schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("extract_session inputSchema should have properties");
    assert!(
        extract_properties.contains_key("repo_path"),
        "extract_session properties should include repo_path for discoverability"
    );
    assert!(
        extract_properties.contains_key("transcript_inline"),
        "extract_session properties should include transcript_inline for discoverability"
    );
    let extract_required = extract_schema
        .get("required")
        .and_then(|r| r.as_array())
        .expect("extract_session inputSchema should have required array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<&str>>();
    assert!(
        extract_required.contains(&"transcript_ref"),
        "extract_session required should include transcript_ref"
    );
    assert!(
        extract_required.contains(&"session_id"),
        "extract_session required should include session_id"
    );
    assert!(
        !extract_required.contains(&"repo_path"),
        "extract_session required should not include repo_path (optional)"
    );
    assert!(
        !extract_required.contains(&"transcript_inline"),
        "extract_session required should not include transcript_inline (optional)"
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

/// Proves that a `compact`-triggered request bypasses session suppression and returns
/// fresh context instead of `DuplicateSuppressed`. This enables compaction re-injection:
/// the first prompt compiles and suppresses; the compaction hook re-invokes with
/// `trigger: TriggerKind::Compact` and must receive `Ok` (not `DuplicateSuppressed`).
#[tokio::test]
async fn compact_trigger_bypasses_suppression_and_returns_fresh_context() {
    let _env_guard = env_guard::configure_scope_env();
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let session_id = "session-compact-bypass";
    let prompt = "how do i read a file in rust";
    let repo_path = test_repo_path();

    // First call: establishes suppression state (Ok response + suppressed flag set).
    let first = server
        .compile_context(CompileContextRequest {
            prompt: prompt.to_owned(),
            session_id: session_id.to_owned(),
            repo_path: repo_path.clone(),
            trigger: None,
        })
        .await;
    assert_eq!(first.status, CompileContextStatus::Ok);

    // Second call: ordinary re-call is suppressed as expected.
    let suppressed = server
        .compile_context(CompileContextRequest {
            prompt: prompt.to_owned(),
            session_id: session_id.to_owned(),
            repo_path: repo_path.clone(),
            trigger: None,
        })
        .await;
    assert_eq!(suppressed.status, CompileContextStatus::DuplicateSuppressed);

    // Third call: compaction re-inject with `trigger: TriggerKind::Compact` must bypass
    // suppression and return fresh context so post-compaction context injection works.
    let compact_reinject = server
        .compile_context(CompileContextRequest {
            prompt: prompt.to_owned(),
            session_id: session_id.to_owned(),
            repo_path: repo_path.clone(),
            trigger: Some(TriggerKind::Compact),
        })
        .await;
    assert_ne!(
        compact_reinject.status,
        CompileContextStatus::DuplicateSuppressed,
        "compact trigger must not return DuplicateSuppressed"
    );
    assert_eq!(
        compact_reinject.status,
        CompileContextStatus::Ok,
        "compact trigger must return fresh context"
    );
    assert!(
        compact_reinject.additional_context.is_some(),
        "compact trigger must include compiled context"
    );
}

/// Proves the three-state `usage_write` observability contract in `compile_context` responses.
///
/// An agent must be able to distinguish:
///   "disabled" — `MCP_USAGE_LOGGING=off` or no writer wired (rollback flag active)
///   "ok"       — writer active, last write succeeded (key absent from response)
///   "failed"   — writer active, last write or channel-post failed
///
/// This test exercises the `disabled` state: when `with_usage_writer` is not called
/// (the default for `with_explicit_graph`), `usage_sender` is `None` and the response
/// must carry `health["usage_write"] = "disabled"`, never an absent key.
#[tokio::test]
async fn compile_context_reports_usage_write_disabled_when_no_writer_is_wired() {
    let _env_guard = env_guard::configure_scope_env();
    // No `.with_usage_writer(...)` call — usage_sender stays None, triggering disabled state.
    let server = McpServerApp::with_explicit_graph(
        Arc::new(DeterministicEmbeddingService::healthy()),
        seeded_graph(),
        retrieval_config(),
        None,
    );

    let response = server
        .compile_context(CompileContextRequest {
            prompt: "how do i read a file in rust".to_owned(),
            session_id: "session-usage-disabled".to_owned(),
            repo_path: test_repo_path(),
            trigger: None,
        })
        .await;

    let usage_write_status = response.health.get("usage_write").map(String::as_str);
    assert_eq!(
        usage_write_status,
        Some("disabled"),
        "compile_context response must include health[\"usage_write\"] = \"disabled\" when no writer is wired; \
         got: {usage_write_status:?}"
    );
}
