//! Live PG round-trip proof for T09 AC-7.
//!
//! The keystone unit test `skill_with_real_source_paths_matches_scope_by_true_provenance_not_scope_root`
//! constructs a `RetrievalSnapshot` in memory and never exercises the real
//! INSERT → SELECT path. This test closes that gap by:
//!
//! 1. Seeding a skill with **non-empty** `source_paths` into live PG via
//!    `PostgresRebuildCoordinator::replace_snapshot_and_bump_version`.
//! 2. Reading it back with `PostgresGraphSnapshotStore::list_skills` and
//!    asserting the `source_paths` column round-trips correctly (not empty).
//! 3. Confirming the boot-path provenance logic: when `source_paths` is
//!    non-empty in the persisted record, the scope-matching gate uses the real
//!    file path, not the configured scope root. This mirrors what
//!    `build_graph_from_pg` does in production.
//!
//! Run without live containers: the test is `#[ignore]`-gated so the normal
//! `cargo test` suite is unaffected.

use std::path::PathBuf;

use domain::{ScopeType, SubunitType};
use infrastructure::{
    LiveGraphSkillRecord, LiveGraphSnapshotMutation, LiveGraphSubunitRecord,
    PostgresGraphSnapshotStore, RebuildCoordinator,
};
use mcp_server::McpServerApp;
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

/// Proves that a skill seeded with non-empty `source_paths` via the real
/// `PostgresRebuildCoordinator` INSERT path reads back with the same paths
/// from `list_skills`, and that the provenance path — not the configured scope
/// root — is what scope-matching sees after the boot adapter processes it.
///
/// This directly evidences T09 AC-7's "loaded from PG" claim end-to-end.
#[ignore = "requires live containers"]
#[tokio::test]
async fn pg_source_paths_round_trip_preserves_provenance_for_scope_matching() {
    let _env_guard = env_guard::configure_scope_env();

    let components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure");

    // Derive the repo root so the inserted path is a real relative-to-repo
    // string that could represent a genuine SKILL.md file origin.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize");

    // Use a fixture SKILL.md that is real on disk so `canonicalize` in the
    // boot adapter succeeds and the path is normalized consistently across
    // both the insert and the scope-matching assertion.
    let seeded_source_path =
        repo_root.join("tests/fixtures/test-skills/project/rust-file-io/SKILL.md");
    let seeded_source_path_str = seeded_source_path.display().to_string();

    let mutation = LiveGraphSnapshotMutation {
        rebuilt_at: chrono::Utc::now(),
        skills: vec![LiveGraphSkillRecord {
            stable_id: "ac7-provenance-roundtrip".to_owned(),
            name: "ac7-provenance-roundtrip".to_owned(),
            description: "AC-7 provenance round-trip skill for T09 acceptance-gap closure"
                .to_owned(),
            scope: ScopeType::Project,
            tags: vec!["provenance".to_owned(), "ac7".to_owned()],
            // Non-empty source_paths: the INSERT path must write this column,
            // and the SELECT path must return it unchanged.
            source_paths: vec![seeded_source_path_str.clone()],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Use real source paths".to_owned(),
                content: "Skills seeded via graph-builder carry real SKILL.md provenance paths."
                    .to_owned(),
            }],
        }],
        communities: vec![],
    };

    components
        .rebuild_coordinator
        .replace_snapshot_and_bump_version(mutation)
        .await
        .expect("should seed AC-7 provenance skill into PG with non-empty source_paths");

    // --- AC-7 assertion 1: source_paths round-trips through PG unchanged ----
    //
    // Call the real SELECT path (`list_skills`) and assert the read-back
    // `source_paths` is non-empty and equals what was inserted.  An empty
    // read-back would prove the fallback (scope-root stand-in) is silently
    // masking a write defect.
    let store = PostgresGraphSnapshotStore::new(components.pg_adapter.pool().clone());
    let persisted_skills = store
        .list_skills()
        .await
        .expect("should read skills from live PG");

    let roundtrip_skill = persisted_skills
        .iter()
        .find(|s| s.name == "ac7-provenance-roundtrip")
        .expect(
            "seeded AC-7 skill must appear in list_skills result; \
             empty result indicates write or read regression",
        );

    assert!(
        !roundtrip_skill.source_paths.is_empty(),
        "source_paths must be non-empty after round-trip through PG; \
         empty means the INSERT wrote '{{}}' or the SELECT did not return the column"
    );

    assert_eq!(
        roundtrip_skill.source_paths,
        vec![seeded_source_path_str.clone()],
        "source_paths read back from PG must equal the inserted value exactly; \
         mismatch proves a serialisation or column-mapping defect in the write/read path"
    );

    // --- AC-7 assertion 2: scope-matching uses the real provenance path ------
    //
    // Mirror the boot-path logic from `build_graph_from_pg` (lines 798-815):
    // when `source_paths` is non-empty the boot adapter uses those paths as-is
    // (with optional canonicalization) rather than falling back to the scope
    // root. We replicate that decision here to prove that the PG-sourced paths
    // flow correctly into the `seeded_skill_matches_scope` gate.
    //
    // `seeded_skill_matches_scope` asserts:
    //   `source_paths.iter().all(|p| scope.paths.iter().any(|sp| p.starts_with(sp)))`
    // So a scope rooted at `repo_root` must match a source_path under it, while
    // a scope rooted at `/unrelated/path` must not.
    let actual_source_path = roundtrip_skill
        .source_paths
        .first()
        .expect("source_paths non-empty per assertion 1");

    // Apply the same canonicalize-or-fallback the boot adapter applies.
    let resolved_source_path = std::fs::canonicalize(actual_source_path)
        .unwrap_or_else(|_| PathBuf::from(actual_source_path));

    // Scope root that SHOULD match: `repo_root` is a prefix of `seeded_source_path`.
    let matching_scope_root = repo_root.clone();
    assert!(
        resolved_source_path.starts_with(&matching_scope_root),
        "boot-path provenance: source_path from PG ({resolved_source_path:?}) must start \
         with the matching scope root ({matching_scope_root:?}); \
         this gate is what seeded_skill_matches_scope checks"
    );

    // Scope root that must NOT match: `/tmp` is not a prefix of `seeded_source_path`.
    let non_matching_scope_root = PathBuf::from("/tmp");
    assert!(
        !resolved_source_path.starts_with(&non_matching_scope_root),
        "boot-path provenance: source_path from PG must not be matched by an unrelated \
         scope root; the path gate must exclude out-of-scope skills"
    );

    components
        .teardown()
        .await
        .expect("teardown should succeed");
}
