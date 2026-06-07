use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use domain::{HdbscanConfig, ScopeType};
use graph_builder::{
    GraphRebuildOrchestrator, PostgresDurableGraphState, ScopeRoot, SkillFileChangeKind,
    graph::build::build_skills_from_scope_roots, watcher::FileChangeSource,
};
use infrastructure::{
    EventEnvelope, OllamaEmbeddingConfig, OllamaEmbeddingService, OutboxRelay, OutboxVectorStore,
    PostgresAdapter, PostgresConfig, PostgresGraphWriteCoordinator, PostgresRebuildCoordinator,
    QdrantAdapter, QdrantConfig, RebuildCoordinator,
};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/test-skills")
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

fn copy_tree(from: &Path, to: &Path) {
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry.expect("fixture entry should load");
        let relative = entry
            .path()
            .strip_prefix(from)
            .expect("fixture path should strip prefix");
        let target = to.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).expect("fixture directory should copy");
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("parent directory should be creatable");
            }
            fs::copy(entry.path(), &target).expect("fixture file should copy");
        }
    }
}

fn db_url() -> String {
    let db = std::env::var("POSTGRES_DB").unwrap_or_else(|_| "skill_layer_test".to_owned());
    let user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "skill_layer".to_owned());
    let pass = std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "skill_layer".to_owned());
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| format!("postgres://{user}:{pass}@localhost:15432/{db}"))
}

fn qdrant_url() -> String {
    std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:16333".to_owned())
}

async fn setup_pg() -> PostgresAdapter {
    PostgresAdapter::connect(&PostgresConfig {
        database_url: db_url(),
        ..PostgresConfig::default()
    })
    .await
    .expect("PG should be reachable")
}

async fn setup_qdrant() -> QdrantAdapter {
    let qdrant = QdrantAdapter::new(
        reqwest::Client::new(),
        QdrantConfig {
            endpoint: qdrant_url(),
            ..QdrantConfig::default()
        },
    )
    .expect("Qdrant should be reachable");
    let _ = qdrant
        .ensure_collection(&qdrant.config.collection_name, 768)
        .await;
    qdrant
}

fn setup_embedding_service() -> OllamaEmbeddingService {
    let base_url = std::env::var("OLLAMA_URL").expect("OLLAMA_URL must be set for live e2e");
    OllamaEmbeddingService::from_config(OllamaEmbeddingConfig {
        base_url,
        model: "nomic-embed-text".to_owned(),
        max_concurrency: 4,
    })
    .expect("OllamaEmbeddingService should build with valid config")
}

#[tokio::test]
#[ignore = "requires live containers"]
async fn graph_builder_rebuild_persists_to_pg_and_enqueues_outbox_events_then_drains_to_qdrant() {
    let pg = setup_pg().await;
    let qdrant = setup_qdrant().await;

    let sandbox = fresh_sandbox("e2e-graph-persist");
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    copy_tree(&fixture_root().join("project"), &project_root);
    copy_tree(&fixture_root().join("global"), &global_root);

    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
        ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
    ];

    let embedding_service = setup_embedding_service();
    let rebuild_coordinator = PostgresRebuildCoordinator::new(pg.pool().clone());
    let outbox_coordinator = PostgresGraphWriteCoordinator::new(pg.pool().clone());
    let mut durable_state =
        PostgresDurableGraphState::new(&rebuild_coordinator, &outbox_coordinator, &qdrant);
    let mut published_events: Vec<EventEnvelope> = Vec::new();
    let mut orchestrator = GraphRebuildOrchestrator::new(
        &mut durable_state,
        &mut published_events,
        &embedding_service,
    );

    let skills = build_skills_from_scope_roots(&scopes, &embedding_service)
        .await
        .expect("build should succeed");
    assert!(!skills.is_empty(), "should discover fixture skills");

    let file_changes: Vec<graph_builder::SkillFileChange> = skills
        .iter()
        .map(|skill| graph_builder::SkillFileChange {
            scope_id: skill.scope_id.clone(),
            scope_type: skill.scope_type,
            file_path: skill.source_path.clone(),
            idempotency_key: format!("e2e-chg-{}", skill.id),
            source: FileChangeSource::Direct,
            kind: SkillFileChangeKind::Modified,
            content_hash: skill.id.clone(),
        })
        .collect();

    let outcome = orchestrator
        .rebuild_from_changes(&scopes, &file_changes, &HdbscanConfig::default())
        .await
        .expect("rebuild should succeed");

    assert!(outcome.graph_version > 0);
    assert_eq!(outcome.skills_count, skills.len());
    assert!(outcome.communities_count > 0);
    assert_eq!(published_events.len(), 1);
    assert_eq!(published_events[0].event_type, "graph.rebuilt");

    let store = infrastructure::PostgresGraphSnapshotStore::new(pg.pool().clone());
    let persisted_skills = store
        .list_skills()
        .await
        .expect("should read skills from PG");
    assert_eq!(
        persisted_skills.len(),
        skills.len(),
        "every built skill should be persisted in PG"
    );

    let point_ids = qdrant
        .list_point_ids()
        .await
        .expect("should list Qdrant points");
    assert!(
        !point_ids.point_ids.is_empty(),
        "outbox drain should have upserted vectors to Qdrant"
    );

    fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");
}

#[tokio::test]
#[ignore = "requires live containers"]
async fn full_roundtrip_filesystem_to_graph_builder_to_pg_and_qdrant() {
    let pg = setup_pg().await;
    let qdrant = setup_qdrant().await;

    let sandbox = fresh_sandbox("e2e-full-roundtrip");
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    copy_tree(&fixture_root().join("project"), &project_root);
    copy_tree(&fixture_root().join("global"), &global_root);

    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
        ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
    ];

    let embedding_service = setup_embedding_service();
    let rebuild_coordinator = PostgresRebuildCoordinator::new(pg.pool().clone());
    let outbox_coordinator = PostgresGraphWriteCoordinator::new(pg.pool().clone());
    let mut durable_state =
        PostgresDurableGraphState::new(&rebuild_coordinator, &outbox_coordinator, &qdrant);
    let mut published_events: Vec<EventEnvelope> = Vec::new();
    let mut orchestrator = GraphRebuildOrchestrator::new(
        &mut durable_state,
        &mut published_events,
        &embedding_service,
    );
    let skills = build_skills_from_scope_roots(&scopes, &embedding_service)
        .await
        .expect("build should succeed");
    let file_changes: Vec<graph_builder::SkillFileChange> = skills
        .iter()
        .map(|skill| graph_builder::SkillFileChange {
            scope_id: skill.scope_id.clone(),
            scope_type: skill.scope_type,
            file_path: skill.source_path.clone(),
            idempotency_key: format!("roundtrip-chg-{}", skill.id),
            source: FileChangeSource::Direct,
            kind: SkillFileChangeKind::Modified,
            content_hash: skill.id.clone(),
        })
        .collect();
    orchestrator
        .rebuild_from_changes(&scopes, &file_changes, &HdbscanConfig::default())
        .await
        .expect("rebuild should succeed");

    let point_ids = qdrant
        .list_point_ids()
        .await
        .expect("should list Qdrant points");
    assert!(!point_ids.point_ids.is_empty());

    let pg_skills = infrastructure::PostgresGraphSnapshotStore::new(pg.pool().clone())
        .list_skills()
        .await
        .expect("should read skills from PG");
    assert!(!pg_skills.is_empty());

    let pg_version = rebuild_coordinator
        .current_graph_version()
        .await
        .expect("should read graph version");
    assert!(pg_version > 0);

    fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");
}

/// Closes #165 acceptance criterion #3 at the PERSISTENCE layer: after a real
/// rebuild (real Ollama embeddings → real HDBSCAN clustering → atomic PG write),
/// the `community_skills` table must carry BOTH membership sources, and at least
/// one skill must hold an `'hdbscan'` row AND a `'tag'` row simultaneously — i.e.
/// dual membership is persisted, not just produced in memory.
///
/// This is robust to clustering outcome: every skill always lands in the tag layer
/// AND in the HDBSCAN layer (a named cluster when it clusters, otherwise the
/// per-scope `-unclustered` community — still `source='hdbscan'`), so a correctly
/// wired write path always yields a skill with both sources. The test fails loudly
/// if migration 006 did not apply or the `source` column is not written.
#[tokio::test]
#[ignore = "requires live containers"]
async fn rebuild_persists_dual_membership_with_both_community_sources() {
    let pg = setup_pg().await;
    let qdrant = setup_qdrant().await;

    let sandbox = fresh_sandbox("e2e-dual-membership");
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    copy_tree(&fixture_root().join("project"), &project_root);
    copy_tree(&fixture_root().join("global"), &global_root);

    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
        ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
    ];

    let embedding_service = setup_embedding_service();
    let rebuild_coordinator = PostgresRebuildCoordinator::new(pg.pool().clone());
    let outbox_coordinator = PostgresGraphWriteCoordinator::new(pg.pool().clone());
    let mut durable_state =
        PostgresDurableGraphState::new(&rebuild_coordinator, &outbox_coordinator, &qdrant);
    let mut published_events: Vec<EventEnvelope> = Vec::new();
    let mut orchestrator = GraphRebuildOrchestrator::new(
        &mut durable_state,
        &mut published_events,
        &embedding_service,
    );

    let skills = build_skills_from_scope_roots(&scopes, &embedding_service)
        .await
        .expect("build should succeed");
    let file_changes: Vec<graph_builder::SkillFileChange> = skills
        .iter()
        .map(|skill| graph_builder::SkillFileChange {
            scope_id: skill.scope_id.clone(),
            scope_type: skill.scope_type,
            file_path: skill.source_path.clone(),
            idempotency_key: format!("dual-chg-{}", skill.id),
            source: FileChangeSource::Direct,
            kind: SkillFileChangeKind::Modified,
            content_hash: skill.id.clone(),
        })
        .collect();

    orchestrator
        .rebuild_from_changes(&scopes, &file_changes, &HdbscanConfig::default())
        .await
        .expect("rebuild should succeed");

    // Both membership sources must be present in the persisted table.
    let (hdbscan_rows,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM community_skills WHERE source = 'hdbscan'")
            .fetch_one(pg.pool())
            .await
            .expect("should count hdbscan membership rows");
    let (tag_rows,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM community_skills WHERE source = 'tag'")
            .fetch_one(pg.pool())
            .await
            .expect("should count tag membership rows");
    assert!(
        hdbscan_rows > 0,
        "community_skills must contain at least one source='hdbscan' row after rebuild"
    );
    assert!(
        tag_rows > 0,
        "community_skills must contain at least one source='tag' row after rebuild"
    );

    // At least one skill must appear under BOTH sources — true dual membership at
    // the persistence boundary.
    let dual: Option<(String,)> = sqlx::query_as(
        "SELECT skill_id::TEXT FROM community_skills WHERE source = 'hdbscan' \
         INTERSECT \
         SELECT skill_id::TEXT FROM community_skills WHERE source = 'tag' \
         LIMIT 1",
    )
    .fetch_optional(pg.pool())
    .await
    .expect("should query dual-membership skills");
    assert!(
        dual.is_some(),
        "at least one skill must hold BOTH an 'hdbscan' and a 'tag' community_skills row \
         (dual membership must be persisted, not only computed in memory)"
    );

    fs::remove_dir_all(&sandbox).expect("sandbox cleanup should succeed");
}

/// Drains any orphaned `pending` outbox events to Qdrant and asserts that
/// Qdrant point count matches PG skill count after the drain completes.
///
/// This test serves as the live acceptance gate for the `relay_all_pending_to_completion`
/// self-heal path added to fix the 234-skill corpus vectorization failure (#223).
/// It does NOT rebuild the corpus — it only relays pending events that were left
/// stuck by a previous failed rebuild.
///
/// Run with:
/// `cargo test --test test_real_infrastructure_e2e -- drain_orphaned_outbox_pending_reaches_qdrant_point_parity --ignored --nocapture`
#[tokio::test]
#[ignore = "requires live containers"]
async fn drain_orphaned_outbox_pending_reaches_qdrant_point_parity() {
    let pg = setup_pg().await;
    let qdrant = setup_qdrant().await;

    let outbox_coordinator = PostgresGraphWriteCoordinator::new(pg.pool().clone());
    let relay = OutboxRelay::new(&outbox_coordinator, &qdrant, 10, 0)
        .expect("outbox relay should initialize for valid contract");

    let before_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE status = 'pending'")
            .fetch_one(pg.pool())
            .await
            .expect("should count pending events before drain");
    println!("pending outbox events before drain: {before_count}");

    let published = relay
        .relay_all_pending_to_completion()
        .await
        .expect("orphaned pending drain must drain to completion (no arbitrary cap)");
    println!("drained {published} events to Qdrant");

    let after_pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE status = 'pending'")
            .fetch_one(pg.pool())
            .await
            .expect("should count pending events after drain");
    assert_eq!(
        after_pending, 0,
        "all pending outbox events must be drained after relay_all_pending_to_completion"
    );

    let pg_skill_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skills")
        .fetch_one(pg.pool())
        .await
        .expect("should count PG skills");

    let qdrant_points = qdrant
        .list_point_ids()
        .await
        .expect("should list Qdrant points");
    let qdrant_count = qdrant_points.point_ids.len() as i64;

    println!("PG skills: {pg_skill_count}, Qdrant points: {qdrant_count}");
    // Qdrant may hold more points than `pg_skill_count` when other rebuilds (from a
    // different DB, e.g. the live graph-builder using `skill_layer`) also write to
    // the shared Qdrant collection. The invariant we prove here is that Qdrant
    // holds AT LEAST as many points as the skills in this DB, confirming the drain
    // pushed all corpus vectors through. A strict equality check would be fragile
    // in multi-DB environments.
    assert!(
        qdrant_count >= pg_skill_count,
        "Qdrant must hold at least as many points as PG skills after full drain: \
         Qdrant={qdrant_count}, PG={pg_skill_count}"
    );
}
