//! Narrow boot-time live-retrieval smoke for T01 (V1.5 "Close the Loop").
//!
//! Proves the production boot path: construct the server from the environment
//! (the renamed `build_live_server`), seed one skill into the live PG graph,
//! reconstruct from the environment so boot reads the seeded graph, and confirm
//! `compile_context` returns `Ok` with the seeded skill — NOT `no_match`.
//!
//! This is intentionally narrower than `test_live_data_plane_roundtrip`: it does
//! not exercise refresh-on-rebuild (T02), Qdrant live query (T03), or duplicate
//! suppression. It only proves that a clean deployment retrieves a skill that
//! exists in the graph at boot time.

use std::path::PathBuf;

use domain::{ScopeType, SubunitType};
use infrastructure::{
    LiveGraphSkillRecord, LiveGraphSnapshotMutation, LiveGraphSubunitRecord, RebuildCoordinator,
};
use mcp_server::{
    McpServerApp,
    tools::compile_context::{CompileContextRequest, CompileContextStatus},
};
use retrieval::RetrievalConfig;

#[path = "../integration/env_guard.rs"]
mod env_guard;

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

fn repo_root_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
        .display()
        .to_string()
}

#[ignore = "requires live containers"]
#[tokio::test]
async fn boot_time_live_retrieval() {
    // Per-run namespace isolation (#164): build all components against a sandbox
    // PG schema / Qdrant collection / Redis stream so the destructive teardowns
    // below never touch the live containers' canonical namespace. Also sets the
    // scope env (folded in to avoid double-locking ENV_LOCK).
    let namespace = env_guard::isolated_namespace().await;

    // Boot once to obtain the live components (PG/Qdrant/Redis/Ollama wiring) and
    // seed a single retrievable skill into the durable graph store.
    let seed_components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure for seeding");

    let mutation = LiveGraphSnapshotMutation {
        rebuilt_at: chrono::Utc::now(),
        skills: vec![LiveGraphSkillRecord {
            stable_id: "boot-time-rust-file-io".to_owned(),
            name: "boot-time-rust-file-io".to_owned(),
            description:
                "Boot-time live retrieval file I/O patterns in Rust with async tokio boundaries"
                    .to_owned(),
            scope: ScopeType::Global,
            tags: vec!["rust".to_owned(), "file".to_owned(), "io".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Read file async".to_owned(),
                content: "Use tokio::fs::read_to_string for small files within async contexts"
                    .to_owned(),
            }],
        }],
        communities: vec![],
    };
    seed_components
        .rebuild_coordinator
        .replace_snapshot_and_bump_version(mutation)
        .await
        .expect("should seed boot-time skill into PG");

    // Reboot from the environment: production boot must read the seeded live graph,
    // not an empty in-memory stub.
    let booted = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure after seeding");

    let response = booted
        .app
        .compile_context(CompileContextRequest {
            // Prompt deliberately echoes the seeded skill so the real Ollama
            // embedding cosine clears the relevance threshold; this smoke proves
            // the boot path serves the live graph, not the ranking tuning (T09).
            prompt:
                "Boot-time live retrieval file I/O patterns in Rust with async tokio boundaries"
                    .to_owned(),
            session_id: "boot-time-live-retrieval".to_owned(),
            repo_path: repo_root_path(),
            trigger: None,
        })
        .await;

    assert_eq!(
        response.status,
        CompileContextStatus::Ok,
        "clean boot must retrieve the seeded skill, got {:?} (reason {:?})",
        response.status,
        response.reason_code
    );
    assert!(
        response
            .additional_context
            .as_deref()
            .unwrap_or("")
            .contains("boot-time-rust-file-io"),
        "compiled context must contain the seeded skill name, got: {:?}",
        response.additional_context
    );

    booted.teardown().await.expect("teardown should succeed");
    seed_components
        .teardown()
        .await
        .expect("teardown should succeed");

    // Drop the sandbox schema / collection / stream. Only touches this run's
    // namespace; the containers' canonical namespace is never affected.
    namespace.cleanup().await;
}
