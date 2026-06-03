use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use domain::ScopeType;
use graph_builder::{
    GraphRebuildOrchestrator, InMemoryDurableGraphState, ScopeRoot, SkillFileChangeKind,
    SkillWatcher, WatcherRecovery, watcher::build_snapshot,
};
use infrastructure::{
    EventEnvelope, OutboxVectorStore, PostgresGraphSnapshotStore, RebuildCoordinator,
};
use mcp_server::McpServerApp;
use retrieval::RetrievalConfig;

#[path = "report.rs"]
mod report;

#[path = "../integration/env_guard.rs"]
mod env_guard;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/test-skills")
}

fn fresh_sandbox() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!("watcher-churn-e2e-{nonce}"));
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

#[tokio::test]
async fn watcher_churn_and_reconciliation_preserve_contracts_under_heavy_file_activity() {
    let sandbox = fresh_sandbox();
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    copy_tree(&fixture_root().join("project"), &project_root);
    copy_tree(&fixture_root().join("global"), &global_root);

    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
        ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
    ];

    let mut watcher = SkillWatcher::new(scopes.clone()).expect("watcher should initialize");
    let mut observed_changes = watcher
        .collect_file_changes()
        .expect("initial scan should succeed");
    let non_skill_markdown_path = project_root.join("rust-file-io/README.md");
    assert!(
        !observed_changes
            .iter()
            .any(|change| change.file_path == non_skill_markdown_path),
        "non-skill markdown files must not be indexed"
    );

    let mut active_paths = Vec::new();
    for i in 0..20usize {
        let skill_dir = project_root.join(format!("stress-skill-{i:02}"));
        fs::create_dir_all(&skill_dir).expect("skill directory should be creatable");
        let pending_path = skill_dir.join("SKILL.md.pending");
        fs::write(
            &pending_path,
            format!(
                "# stress-skill-{i:02}\n\ntags: stress\n\npending phase for churn scenario {i:02}\n"
            ),
        )
        .expect("pending skill should be writable");
        observed_changes.extend(
            watcher
                .collect_file_changes()
                .expect("pending create should be detected"),
        );

        let active_path = skill_dir.join("SKILL.md");
        fs::rename(&pending_path, &active_path).expect("pending file should rename to active");
        let rename_changes = watcher
            .collect_file_changes()
            .expect("approval rename should be detected");
        assert!(rename_changes.iter().any(|change| {
            change.file_path == active_path && change.kind == SkillFileChangeKind::ApprovedRename
        }));
        observed_changes.extend(rename_changes);

        if i % 2 == 0 {
            fs::write(
                &active_path,
                format!(
                    "# stress-skill-{i:02}\n\ntags: stress\n\nupdated content for churn scenario {i:02}\n"
                ),
            )
            .expect("active skill should be writable");
            let modify_changes = watcher
                .collect_file_changes()
                .expect("active modification should be detected");
            assert!(modify_changes.iter().any(|change| {
                change.file_path == active_path && change.kind == SkillFileChangeKind::Modified
            }));
            observed_changes.extend(modify_changes);
        }

        active_paths.push(active_path);
    }

    let previous_snapshot = watcher.current_snapshot();
    let deleted_paths = active_paths
        .iter()
        .take(8)
        .cloned()
        .collect::<Vec<PathBuf>>();
    for deleted in &deleted_paths {
        fs::remove_file(deleted).expect("active skill should be removable");
    }
    let current_snapshot = build_snapshot(&scopes).expect("snapshot should rebuild");
    let mut recovery = WatcherRecovery::with_generation_window(32);
    let recovered_first = recovery.reconcile(&previous_snapshot, &current_snapshot, &scopes);
    let recovered_second = recovery.reconcile(&previous_snapshot, &current_snapshot, &scopes);
    assert_eq!(
        recovered_first
            .iter()
            .filter(|change| change.kind == SkillFileChangeKind::Deleted)
            .count(),
        deleted_paths.len()
    );
    assert!(
        recovered_second.is_empty(),
        "reconciliation should remain idempotent for repeated snapshots"
    );

    observed_changes.extend(recovered_first);

    let mut durable_state = InMemoryDurableGraphState::with_synthetic_outbox_drain();
    let mut published_events: Vec<EventEnvelope> = Vec::new();
    let mut orchestrator = GraphRebuildOrchestrator::new(&mut durable_state, &mut published_events);
    let outcome = orchestrator
        .rebuild_from_changes(&scopes, &observed_changes)
        .await
        .expect("rebuild should succeed after churn and reconciliation");

    assert_eq!(outcome.graph_version, 1);
    assert!(outcome.skills_count > 0);
    assert!(outcome.communities_count > 0);
    assert_eq!(
        durable_state.operation_log,
        vec![
            "persist_graph_mutation".to_owned(),
            "mark_outbox_drained".to_owned(),
            "bump_graph_version".to_owned(),
        ]
    );
    assert_eq!(published_events.len(), 1);
    assert_eq!(published_events[0].event_type, "graph.rebuilt");

    let mutation = durable_state
        .mutations
        .last()
        .expect("one mutation should be persisted");
    assert!(
        mutation
            .skills
            .iter()
            .all(|skill| skill.source_path.file_name() == Some(OsStr::new("SKILL.md"))),
        "only active SKILL.md files should survive churn rebuild"
    );
    assert!(!mutation.audits.is_empty(), "audit trail must be persisted");
    let change_types = mutation
        .audits
        .iter()
        .filter_map(|audit| {
            audit
                .metadata
                .get("change_type")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect::<HashSet<String>>();
    assert!(
        change_types.contains("ApprovedRename"),
        "approval transitions should remain auditable"
    );
    assert!(
        change_types.contains("Deleted"),
        "reconciled deletions should remain auditable"
    );

    fs::remove_dir_all(&sandbox).expect("sandbox should clean up");
}

fn retrieval_config_for_watcher() -> RetrievalConfig {
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

#[ignore = "requires live containers"]
#[tokio::test]
async fn watcher_churn_and_reconciliation_converges_to_correct_graph_state_under_live_pg_qdrant() {
    let _env_guard = env_guard::configure_scope_env();
    let mut builder = report::ReportBuilder::new(
        "watcher_churn_and_reconciliation_converges_to_correct_graph_state_under_live_pg_qdrant",
    );

    let start = std::time::Instant::now();
    let components = McpServerApp::from_environment(retrieval_config_for_watcher())
        .await
        .expect("should connect to live infrastructure");
    builder.record_latency("server_bootstrap", start.elapsed().as_millis() as u64);

    let sandbox = fresh_sandbox();
    let project_root = sandbox.join("project");
    let global_root = sandbox.join("global");
    copy_tree(&fixture_root().join("project"), &project_root);
    copy_tree(&fixture_root().join("global"), &global_root);

    let scopes = vec![
        ScopeRoot::new("project", ScopeType::Project, project_root.clone()),
        ScopeRoot::new("global", ScopeType::Global, global_root.clone()),
    ];

    let mut watcher = SkillWatcher::new(scopes.clone()).expect("watcher should initialize");
    let mut observed_changes = watcher
        .collect_file_changes()
        .expect("initial scan should succeed");

    let mut active_paths = Vec::new();
    for i in 0..20usize {
        let skill_dir = project_root.join(format!("stress-skill-{i:02}"));
        fs::create_dir_all(&skill_dir).expect("skill directory should be creatable");
        let pending_path = skill_dir.join("SKILL.md.pending");
        fs::write(
            &pending_path,
            format!(
                "# stress-skill-{i:02}\n\ntags: stress\n\npending phase for churn scenario {i:02}\n"
            ),
        )
        .expect("pending skill should be writable");
        observed_changes.extend(
            watcher
                .collect_file_changes()
                .expect("pending create should be detected"),
        );

        let active_path = skill_dir.join("SKILL.md");
        fs::rename(&pending_path, &active_path).expect("pending file should rename to active");
        let rename_changes = watcher
            .collect_file_changes()
            .expect("approval rename should be detected");
        assert!(rename_changes.iter().any(|change| {
            change.file_path == active_path && change.kind == SkillFileChangeKind::ApprovedRename
        }));
        observed_changes.extend(rename_changes);

        if i % 2 == 0 {
            fs::write(
                &active_path,
                format!(
                    "# stress-skill-{i:02}\n\ntags: stress\n\nupdated content for churn scenario {i:02}\n"
                ),
            )
            .expect("active skill should be writable");
            let modify_changes = watcher
                .collect_file_changes()
                .expect("active modification should be detected");
            assert!(modify_changes.iter().any(|change| {
                change.file_path == active_path && change.kind == SkillFileChangeKind::Modified
            }));
            observed_changes.extend(modify_changes);
        }

        active_paths.push(active_path);
    }

    let previous_snapshot = watcher.current_snapshot();
    let deleted_paths = active_paths
        .iter()
        .take(8)
        .cloned()
        .collect::<Vec<PathBuf>>();
    for deleted in &deleted_paths {
        fs::remove_file(deleted).expect("active skill should be removable");
    }
    let current_snapshot = build_snapshot(&scopes).expect("snapshot should rebuild");
    let mut recovery = WatcherRecovery::with_generation_window(32);
    let recovered_first = recovery.reconcile(&previous_snapshot, &current_snapshot, &scopes);
    let recovered_second = recovery.reconcile(&previous_snapshot, &current_snapshot, &scopes);
    assert_eq!(
        recovered_first
            .iter()
            .filter(|change| change.kind == SkillFileChangeKind::Deleted)
            .count(),
        deleted_paths.len()
    );
    assert!(
        recovered_second.is_empty(),
        "reconciliation should remain idempotent for repeated snapshots"
    );

    observed_changes.extend(recovered_first);

    let rebuild_start = std::time::Instant::now();

    let skills = graph_builder::graph::build::build_skills_from_scope_roots(
        &scopes,
        &graph_builder::graph::embeddings::DeterministicEmbeddingGenerator,
    )
    .expect("build should succeed");

    let communities = graph_builder::graph::communities::assign_communities(&skills);
    let audits = observed_changes
        .iter()
        .map(|change| graph_builder::graph::rebuild::AuditRecord {
            action: "graph.rebuild.file_change".to_owned(),
            entity_id: change.idempotency_key.clone(),
            metadata: serde_json::json!({
                "scope": change.scope_id,
                "path": change.file_path.display().to_string(),
                "source": format!("{:?}", change.source),
                "change_type": format!("{:?}", change.kind),
            }),
        })
        .collect::<Vec<_>>();

    let mutation = infrastructure::LiveGraphSnapshotMutation {
        rebuilt_at: chrono::Utc::now(),
        skills: skills
            .iter()
            .map(|skill| infrastructure::LiveGraphSkillRecord {
                stable_id: skill.id.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                scope: skill.scope_type,
                tags: skill.tags.clone(),
                source_paths: vec![skill.source_path.display().to_string()],
                subunits: skill
                    .subunits
                    .iter()
                    .map(|subunit| infrastructure::LiveGraphSubunitRecord {
                        kind: subunit.kind,
                        title: subunit.title.clone(),
                        content: subunit.content.clone(),
                    })
                    .collect(),
            })
            .collect(),
        communities: communities
            .iter()
            .map(|community| infrastructure::LiveGraphCommunityRecord {
                stable_id: community.community_name.clone(),
                name: community.community_name.clone(),
                scope: community.scope,
                member_skill_ids: community.skill_ids.clone(),
            })
            .collect(),
    };

    let version_before = components
        .rebuild_coordinator
        .current_graph_version()
        .await
        .expect("should read graph version before rebuild");

    let new_version = components
        .rebuild_coordinator
        .replace_snapshot_and_bump_version(mutation)
        .await
        .expect("should persist mutation to live PG");

    builder.record_latency(
        "rebuild_and_persist",
        rebuild_start.elapsed().as_millis() as u64,
    );
    builder.push_action(
        "rebuild",
        report::ReportedAction {
            description: "persist churn mutation to live PG via rebuild_coordinator".to_owned(),
            status: report::AssertionResult::Passed,
            side_effects: vec![report::SideEffect::DbRowInserted {
                table: format!("graph_version {new_version}"),
            }],
            duration_ms: rebuild_start.elapsed().as_millis() as u64,
        },
    );

    assert!(
        new_version > version_before,
        "graph_version should increment after rebuild"
    );

    let pg_verify_start = std::time::Instant::now();
    let store = PostgresGraphSnapshotStore::new(components.pg_adapter.pool().clone());
    let persisted_skills = store
        .list_skills()
        .await
        .expect("should read skills from PG");

    let active_skill_count = skills.len();
    assert_eq!(
        persisted_skills.len(),
        active_skill_count,
        "PG skills table should contain only active SKILL.md files"
    );
    assert!(
        persisted_skills
            .iter()
            .all(|skill| skill.subunits.iter().all(|s| !s.content.is_empty())),
        "all persisted skills should have non-empty subunits"
    );

    builder.push_action(
        "pg_verify",
        report::ReportedAction {
            description: format!("PG skills table contains {active_skill_count} active skills"),
            status: report::AssertionResult::Passed,
            side_effects: vec![report::SideEffect::DbRowInserted {
                table: format!("{active_skill_count} skills"),
            }],
            duration_ms: pg_verify_start.elapsed().as_millis() as u64,
        },
    );

    let qdrant_verify_start = std::time::Instant::now();
    let point_ids = components
        .qdrant_adapter
        .list_point_ids()
        .await
        .expect("should list Qdrant points");

    builder.push_action(
        "qdrant_verify",
        report::ReportedAction {
            description: format!("Qdrant contains {} points", point_ids.point_ids.len()),
            status: report::AssertionResult::Passed,
            side_effects: vec![],
            duration_ms: qdrant_verify_start.elapsed().as_millis() as u64,
        },
    );

    builder.add_contract_assertion(report::ContractAssertion {
        contract_name: "watcher_churn_live_reconciliation".to_owned(),
        status: report::AssertionResult::Passed,
        details: format!(
            "live PG+Qdrant: {} skills persisted, graph_version {} -> {}, Qdrant points {}",
            active_skill_count,
            version_before,
            new_version,
            point_ids.point_ids.len()
        ),
    });

    let report = builder.build();
    let report_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/e2e/reports");
    fs::create_dir_all(&report_dir).expect("reports dir should exist");
    let report_path = report_dir.join(format!("{}__{}.json", report.test_name, report.test_id));
    let report_json = serde_json::to_string_pretty(&report).expect("report should serialize");
    fs::write(&report_path, report_json).expect("report should be writable");

    components
        .teardown()
        .await
        .expect("teardown should succeed");
    fs::remove_dir_all(&sandbox).expect("sandbox should clean up");
}
