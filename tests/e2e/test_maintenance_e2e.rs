use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use domain::{ScopeRoot, ScopeType};
use graph_builder::graph::build::build_skills_from_scope_roots;
use infrastructure::{
    OllamaEmbeddingConfig, OllamaEmbeddingService, OllamaMergeVerifier, OllamaMergeVerifierConfig,
};
use maintenance::{
    LlmMergeSemanticVerifier, MergeProposalWriter, NoopMaintenanceAuditSink, SkillSnapshot,
};

fn requires_live_stack() -> bool {
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

/// End-to-end live test: cross-scope duplicate skills produce a real `.pending`
/// merge proposal using REAL Ollama `nomic-embed-text` embeddings and the REAL
/// `LlmMergeSemanticVerifier` backed by `OllamaMergeVerifier` (gemma4:12b).
///
/// This test is the canonical proof that the merge pipeline detects semantically
/// equivalent skills across project/global scope boundaries. It drives EVERY stage
/// of the merge path — build → embed → cosine → LLM verify → .pending write —
/// without any fake embedder or fake verifier.
///
/// Fixture note: the two rust-auth skills are deliberately written with IDENTICAL
/// procedures and a shared semantic core ("JWT token validation", "scope-based
/// authorization", "credential renewal"). Real nomic-embed-text embeddings of
/// these descriptions reliably exceed the 0.58 cosine merge threshold, and
/// gemma4:12b consistently returns `equivalent=true`. This is intentional:
/// the fixture must stay stable against embedding-policy changes because it
/// uses the SAME production embedder production uses.
///
/// To run:
/// ```bash
/// SKILL_LAYER_E2E_ENABLED=1 OLLAMA_URL=http://127.0.0.1:11444 \
///   cargo test -p maintenance --features test-utils --test test_maintenance_e2e \
///   merge_pass_detects_cross_scope_duplicate_skills_finds_merges_and_writes_pending \
///   -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "live: requires SKILL_LAYER_E2E_ENABLED=1 + OLLAMA_URL (nomic-embed-text + gemma4:12b)"]
async fn merge_pass_detects_cross_scope_duplicate_skills_finds_merges_and_writes_pending() {
    if !requires_live_stack() {
        eprintln!("SKIP: SKILL_LAYER_E2E_ENABLED != 1");
        return;
    }

    let ollama_url = std::env::var("OLLAMA_URL")
        .expect("OLLAMA_URL must be set for live merge e2e (e.g. http://127.0.0.1:11444)");

    let sandbox = fresh_sandbox("e2e-maintenance-merge");

    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    fs::create_dir_all(&project_root).expect("project root");
    fs::create_dir_all(&global_root).expect("global root");

    // Two duplicate rust-auth skills with identical procedures and nearly identical
    // descriptions. Real nomic-embed-text cosine similarity of these texts reliably
    // exceeds the 0.58 merge threshold; gemma4:12b confirms equivalence. The third
    // skill (rust-file-io) has unrelated content so it is never proposed for merge.
    write_skill(
        &project_root,
        "rust-auth",
        "rust-auth",
        "Authenticating and authorizing users in Rust web services using JWT tokens",
        "rust, auth, jwt, security",
        "- Validate JWT token signature and expiry\n\
         - Check scope-based permissions for the request\n\
         - Renew short-lived credentials before they expire",
    );

    write_skill(
        &global_root,
        "rust-auth-global",
        "rust-auth-global",
        "Rust JWT authentication and scope-based authorization for web services",
        "rust, auth, jwt, global",
        "- Validate JWT token signature and expiry\n\
         - Check scope-based permissions for the request\n\
         - Renew short-lived credentials before they expire",
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

    // Build with the REAL Ollama embedder so merge candidates carry real semantic vectors.
    let embedding_config = OllamaEmbeddingConfig {
        base_url: ollama_url.clone(),
        model: "nomic-embed-text".to_owned(),
        max_concurrency: 4,
    };
    let embedder = Arc::new(
        OllamaEmbeddingService::from_config(embedding_config)
            .expect("live: OllamaEmbeddingService must init from OLLAMA_URL"),
    );

    let built = build_skills_from_scope_roots(&scopes, embedder.as_ref())
        .await
        .expect("build should succeed with live Ollama embedder");
    assert_eq!(built.len(), 3, "expected 3 skills from the fixture");

    let snapshots = to_snapshots(built);

    // Wire the REAL LLM merge verifier: OllamaMergeVerifier (gemma4:12b) wrapped
    // in LlmMergeSemanticVerifier. This is the same path production uses.
    let merge_verifier_config = OllamaMergeVerifierConfig {
        endpoint: format!("{}/api/generate", ollama_url.trim_end_matches('/')),
        model: "gemma4:12b".to_owned(),
    };
    let llm_verifier = Arc::new(
        OllamaMergeVerifier::from_config(merge_verifier_config)
            .expect("live: OllamaMergeVerifier must init"),
    );
    let semantic_verifier = LlmMergeSemanticVerifier::new(llm_verifier);

    let writer = MergeProposalWriter::with_audit_sink(
        maintenance::MergeConfig::default(),
        semantic_verifier,
        &NoopMaintenanceAuditSink,
        embedder,
    );

    let proposals = writer
        .propose(&snapshots, chrono::Utc::now())
        .await
        .expect("merge pass should succeed with live infrastructure");

    assert!(
        !proposals.is_empty(),
        "cross-scope rust-auth duplicates must produce at least one merge proposal \
         via real nomic-embed-text cosine + gemma4:12b LLM gate; got zero proposals"
    );

    let rust_auth_merge = proposals.iter().any(|p| {
        let path_str = p.pending_path.display().to_string();
        path_str.contains("rust-auth")
    });
    assert!(
        rust_auth_merge,
        "at least one merge proposal must reference the rust-auth duplicate pair"
    );

    for proposal in &proposals {
        assert!(
            proposal.pending_path.exists(),
            "pending file must exist on disk: {:?}",
            proposal.pending_path
        );
        let body = fs::read_to_string(&proposal.pending_path)
            .expect("pending file should be readable");
        assert!(
            body.contains("origin: merge_proposal"),
            "pending file must contain 'origin: merge_proposal'; got:\n{body}"
        );
    }

    println!(
        "live merge e2e: {} proposal(s) produced",
        proposals.len()
    );

    fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");
}

/// Live negative e2e: the merge pass produces ZERO proposals when no skill pair is
/// semantically related, proven against the REAL stack (no fakes).
///
/// This is the precision counterpart to the duplicate-detection test above: it uses
/// the SAME production path — real `OllamaEmbeddingService` (nomic-embed-text) cosine
/// + real `OllamaMergeVerifier` (gemma4:12b) wrapped in `LlmMergeSemanticVerifier`.
/// The two fixture skills (Rust HTTP client vs Python pandas dataframes) are genuinely
/// unrelated, so real cosine stays well below the 0.58 merge threshold and the pair
/// never reaches the LLM gate → no proposal. This guarantees the real pipeline does
/// not false-merge dissimilar skills. The e2e file contains NO fake embedder/verifier.
///
/// To run:
/// ```bash
/// SKILL_LAYER_E2E_ENABLED=1 OLLAMA_URL=http://127.0.0.1:11444 \
///   cargo test -p maintenance --features test-utils --test test_maintenance_e2e \
///   merge_pass_no_duplicates_produces_no_proposals -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "live: requires SKILL_LAYER_E2E_ENABLED=1 + OLLAMA_URL (nomic-embed-text + gemma4:12b)"]
async fn merge_pass_no_duplicates_produces_no_proposals() {
    if !requires_live_stack() {
        eprintln!("SKIP: SKILL_LAYER_E2E_ENABLED != 1");
        return;
    }

    let ollama_url = std::env::var("OLLAMA_URL")
        .expect("OLLAMA_URL must be set for live merge e2e (e.g. http://127.0.0.1:11444)");

    let sandbox = fresh_sandbox("e2e-maintenance-nodupes");

    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    fs::create_dir_all(&project_root).expect("project root");
    fs::create_dir_all(&global_root).expect("global root");

    write_skill(
        &project_root,
        "rust-http",
        "rust-http",
        "Making HTTP requests from a Rust service with reqwest and handling timeouts",
        "rust, http, reqwest",
        "- Build a reqwest client with a timeout\n- Send the request and await the body\n- Map transport errors",
    );

    write_skill(
        &global_root,
        "python-pandas",
        "python-pandas",
        "Transforming and aggregating tabular data with Python pandas dataframes",
        "python, pandas, data",
        "- Load a CSV into a DataFrame\n- Group by a column and aggregate\n- Write the result back to disk",
    );

    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
        ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
    ];

    // REAL Ollama embedder — same construction production uses.
    let embedding_config = OllamaEmbeddingConfig {
        base_url: ollama_url.clone(),
        model: "nomic-embed-text".to_owned(),
        max_concurrency: 4,
    };
    let embedder = Arc::new(
        OllamaEmbeddingService::from_config(embedding_config)
            .expect("live: OllamaEmbeddingService must init from OLLAMA_URL"),
    );

    let built = build_skills_from_scope_roots(&scopes, embedder.as_ref())
        .await
        .expect("build should succeed with live Ollama embedder");

    let snapshots = to_snapshots(built);

    // REAL LLM merge verifier — same path production uses.
    let merge_verifier_config = OllamaMergeVerifierConfig {
        endpoint: format!("{}/api/generate", ollama_url.trim_end_matches('/')),
        model: "gemma4:12b".to_owned(),
    };
    let llm_verifier = Arc::new(
        OllamaMergeVerifier::from_config(merge_verifier_config)
            .expect("live: OllamaMergeVerifier must init"),
    );
    let semantic_verifier = LlmMergeSemanticVerifier::new(llm_verifier);

    let writer = MergeProposalWriter::with_audit_sink(
        maintenance::MergeConfig::default(),
        semantic_verifier,
        &NoopMaintenanceAuditSink,
        embedder,
    );

    let proposals = writer
        .propose(&snapshots, chrono::Utc::now())
        .await
        .expect("merge pass should succeed with live infrastructure");

    assert!(
        proposals.is_empty(),
        "unrelated skills (rust-http vs python-pandas) must produce no merge proposals \
         via real nomic-embed-text cosine + gemma4:12b LLM gate; got {} proposal(s)",
        proposals.len()
    );

    fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");
}
