use std::{
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
    let sandbox = std::env::temp_dir().join(format!("graph-builder-test-{nonce}"));
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
async fn watcher_detects_pending_approval_and_rebuild_respects_invalidation_order() {
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
    let initial_changes = watcher
        .collect_file_changes()
        .expect("initial scan should succeed");
    assert!(
        initial_changes
            .iter()
            .any(|change| change.scope_id == "project")
    );
    assert!(
        initial_changes
            .iter()
            .any(|change| change.scope_id == "global")
    );
    let non_skill_markdown_path = project_root.join("rust-file-io/README.md");
    assert!(
        !initial_changes
            .iter()
            .any(|change| change.file_path == non_skill_markdown_path),
        "non-skill markdown files should not be indexed"
    );

    let pending_path = project_root.join("approved-skill/SKILL.md.pending");
    fs::create_dir_all(
        pending_path
            .parent()
            .expect("approved skill pending file should have a parent"),
    )
    .expect("approved skill directory should be creatable");
    fs::write(
        &pending_path,
        "# approved-skill\n\ntags: approved\n\nCreated via pending approval.\n",
    )
    .expect("pending skill should be writable");
    let _ = watcher
        .collect_file_changes()
        .expect("pending create change should be observed");

    let approved_path = project_root.join("approved-skill/SKILL.md");
    fs::rename(&pending_path, &approved_path).expect("pending file should rename to markdown");
    let rename_changes = watcher
        .collect_file_changes()
        .expect("rename should be detected");
    assert!(rename_changes.iter().any(|change| {
        change.file_path == approved_path && change.kind == SkillFileChangeKind::ApprovedRename
    }));

    let deleted_path = global_root.join("async-tokio/SKILL.md");
    let previous_snapshot = watcher.current_snapshot();
    fs::remove_file(&deleted_path).expect("global skill should be removable");
    let current_snapshot = build_snapshot(&scopes).expect("snapshot should rebuild");
    let mut recovery = WatcherRecovery::default();
    let recovered_first = recovery.reconcile(&previous_snapshot, &current_snapshot, &scopes);
    let recovered_second = recovery.reconcile(&previous_snapshot, &current_snapshot, &scopes);
    assert!(
        recovered_first
            .iter()
            .any(|change| change.kind == SkillFileChangeKind::Deleted)
    );
    assert!(
        recovered_second.is_empty(),
        "reconciliation should be idempotent for repeated snapshots"
    );

    let mut durable_state = InMemoryDurableGraphState::with_synthetic_outbox_drain();
    let mut published_events: Vec<EventEnvelope> = Vec::new();
    let mut orchestrator = GraphRebuildOrchestrator::new(&mut durable_state, &mut published_events);
    let mut all_changes = rename_changes;
    all_changes.extend(recovered_first);

    let outcome = orchestrator
        .rebuild_from_changes(&scopes, &all_changes)
        .await
        .expect("rebuild should succeed");
    assert_eq!(outcome.graph_version, 1);
    assert_eq!(outcome.skills_count, 2);
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
            .all(|skill| !skill.embedding.is_empty()),
        "embeddings should be durable for all rebuilt skills"
    );
    assert!(
        mutation
            .skills
            .iter()
            .all(|skill| skill.source_path.file_name() == Some(OsStr::new("SKILL.md"))),
        "rebuild should include only active skill contract files"
    );
    assert!(
        !mutation.audits.is_empty(),
        "file-driven changes must persist audit records"
    );

    fs::remove_dir_all(&sandbox).expect("sandbox should clean up");
}
