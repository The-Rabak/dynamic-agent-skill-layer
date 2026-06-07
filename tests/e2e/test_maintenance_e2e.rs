use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use domain::{ScopeRoot, ScopeType};
use graph_builder::graph::build::build_skills_from_scope_roots;
use maintenance::{
    MergeProposalWriter, MergeSemanticVerifier, NoopMaintenanceAuditSink, SkillSnapshot,
};

/// Returns a deterministic embedder suitable for e2e pipeline tests that do not
/// have a live Ollama instance. This is the same body-inclusive candidate embedder
/// the production merge pass uses, but backed by deterministic hash-based vectors.
fn test_candidate_embedder() -> Arc<dyn domain::EmbeddingService> {
    Arc::new(graph_builder::graph::embeddings::DeterministicEmbeddingService)
}

fn _requires_docker_services() -> bool {
    std::env::var("SKILL_LAYER_E2E_ENABLED").is_ok_and(|v| v == "1")
}

fn fresh_sandbox(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    fs::create_dir_all(&sandbox).expect("sandbox should be creatable");
    sandbox
}

fn write_skill(
    root: &std::path::Path,
    dir: &str,
    name: &str,
    desc: &str,
    tags: &str,
    procedures: &str,
) {
    let skill_dir = root.join(dir);
    fs::create_dir_all(&skill_dir).expect("skill dir should exist");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("# {name}\n\ntags: {tags}\n\n{desc}\n\n## Procedures\n{procedures}\n"),
    )
    .expect("skill file should be writable");
}

fn to_snapshots(skills: Vec<graph_builder::graph::build::BuiltSkill>) -> Vec<SkillSnapshot> {
    skills
        .into_iter()
        .map(|skill| SkillSnapshot {
            id: skill.id,
            name: skill.name,
            description: skill.description,
            scope: skill.scope_type,
            source_path: skill.source_path,
            tags: skill.tags,
            subunits: skill.subunits.into_iter().map(|s| s.content).collect(),
            embedding: skill.embedding,
        })
        .collect()
}

/// Always-equivalent verifier used in e2e tests that don't have a live LLM.
///
/// These tests verify the merge *pipeline* (skill loading, proposal writing,
/// filesystem output) — not the LLM gate. A live verifier would require a running
/// Ollama instance, which is not guaranteed in this test environment.
#[derive(Clone)]
struct AlwaysEquivalentVerifier;

#[async_trait]
impl MergeSemanticVerifier for AlwaysEquivalentVerifier {
    async fn are_equivalent(
        &self,
        _left: &SkillSnapshot,
        _right: &SkillSnapshot,
    ) -> Result<bool, maintenance::MergeError> {
        Ok(true)
    }
}

#[tokio::test]
async fn merge_pass_detects_cross_scope_duplicate_skills_finds_merges_and_writes_pending() {
    let sandbox = fresh_sandbox("e2e-maintenance-merge");

    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    fs::create_dir_all(&project_root).expect("project root");
    fs::create_dir_all(&global_root).expect("global root");

    write_skill(
        &project_root,
        "rust-auth",
        "rust-auth",
        "Rust authentication and authorization workflow",
        "rust, auth, security",
        "- Validate JWT tokens\n- Check scope permissions\n- Renew short-lived credentials",
    );

    write_skill(
        &global_root,
        "rust-auth-global",
        "rust-auth-global",
        "Global Rust auth patterns for distributed systems",
        "rust, auth, global",
        "- Validate JWT tokens across services\n- Check scope permissions\n- Renew credentials",
    );

    write_skill(
        &project_root,
        "rust-file-io",
        "rust-file-io",
        "Rust file I/O safety patterns",
        "rust, file, io",
        "- Open file with error handling\n- Read and buffer safely\n- Close with RAII",
    );

    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
        ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
    ];

    let embedder = graph_builder::graph::embeddings::DeterministicEmbeddingService;
    let built = build_skills_from_scope_roots(&scopes, &embedder)
        .await
        .expect("build should succeed");
    assert_eq!(built.len(), 3);

    let snapshots = to_snapshots(built);
    // Use MergeConfig::default() (merge_candidate_threshold = 0.58) with the body-inclusive
    // merge vector. The two rust-auth skills have near-identical BODIES (shared procedures:
    // "Validate JWT tokens", "Check scope permissions", "Renew credentials") but divergent
    // summaries ("authentication and authorization workflow" vs "patterns for distributed
    // systems"). The body-inclusive vector catches this case — it is merge's OWN dedup
    // signal, distinct from the summary-only retrieval ℓ₁ vector in graph-builder. The
    // divergent-summary / shared-body case is exactly what this path must catch.
    let writer = MergeProposalWriter::with_audit_sink(
        maintenance::MergeConfig::default(),
        AlwaysEquivalentVerifier,
        &NoopMaintenanceAuditSink,
        test_candidate_embedder(),
    );

    let proposals = writer
        .propose(&snapshots, chrono::Utc::now())
        .await
        .expect("merge pass should succeed");

    assert!(
        !proposals.is_empty(),
        "cross-scope rust-auth duplicates should produce a merge proposal"
    );

    let rust_auth_merge = proposals.iter().any(|p| {
        let path_str = p.pending_path.display().to_string();
        path_str.contains("rust-auth")
    });
    assert!(
        rust_auth_merge,
        "cross-scope rust-auth duplicates should produce a merge proposal"
    );

    for proposal in &proposals {
        assert!(proposal.pending_path.exists());
        let body =
            fs::read_to_string(&proposal.pending_path).expect("pending file should be readable");
        assert!(body.contains("origin: merge_proposal"));
    }

    fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");
}

#[tokio::test]
async fn merge_pass_no_duplicates_produces_no_proposals() {
    let sandbox = fresh_sandbox("e2e-maintenance-nodupes");

    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    fs::create_dir_all(&project_root).expect("project root");
    fs::create_dir_all(&global_root).expect("global root");

    write_skill(
        &project_root,
        "rust-http",
        "rust-http",
        "Rust HTTP client patterns",
        "rust, http",
        "- Use reqwest\n- Handle timeouts",
    );

    write_skill(
        &global_root,
        "python-async",
        "python-async",
        "Python async patterns",
        "python, async",
        "- Use asyncio\n- Await properly",
    );

    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
        ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
    ];

    let embedder = graph_builder::graph::embeddings::DeterministicEmbeddingService;
    let built = build_skills_from_scope_roots(&scopes, &embedder)
        .await
        .expect("build should succeed");

    let snapshots = to_snapshots(built);
    let writer = MergeProposalWriter::with_audit_sink(
        maintenance::MergeConfig::default(),
        AlwaysEquivalentVerifier,
        &NoopMaintenanceAuditSink,
        test_candidate_embedder(),
    );

    let proposals = writer
        .propose(&snapshots, chrono::Utc::now())
        .await
        .expect("merge pass should succeed");

    assert!(
        proposals.is_empty(),
        "no cross-scope duplicates should produce no merge proposals"
    );

    fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");
}
