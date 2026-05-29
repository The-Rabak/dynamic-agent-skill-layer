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
use infrastructure::EventEnvelope;

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
